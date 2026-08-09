//! Benchmark and calibrate local model profiles against typed fixtures.
//!
//! The binary never pulls a model. Verdicts are calibration data: process exit
//! reflects operational failures only, not a failed calibration.

use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use cortex_eval::backend::{EvalBackend, OllamaEvalBackend};
use cortex_eval::fixtures::default_fixtures;
use cortex_eval::openai_backend::OpenAiEvalBackend;
use cortex_eval::report::{EvalReport, render_markdown, write_json};
use cortex_eval::runner::{
    EmbeddingProfile, EvalProfile, SuiteSelection, run_embedding_profile, run_profile,
};
use cortex_eval::sequence_suite::{SequenceLiveOptions, run_sequence_suite};
use cortex_eval::{EvalError, PROMPT_VERSION, SCHEMA_VERSION};
use cortex_ollama::{ModelProfile, OllamaClient, OllamaConfig};
use cortex_router::ModelTier;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EvalConfigFile {
    base_url: Option<String>,
    /// Per-call ceiling covering cold model loads and slow CPU generation.
    timeout_secs: Option<u64>,
    profiles: Vec<ProfileConfig>,
    #[serde(default)]
    embedding_profiles: Vec<EmbeddingProfileConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmbeddingProfileConfig {
    id: String,
    model: String,
    max_input_tokens: u32,
}

const DEFAULT_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileConfig {
    id: String,
    tier: ModelTier,
    model: String,
    max_input_tokens: u32,
    max_output_tokens: u32,
    context_tokens: u32,
}

struct CliOptions {
    config: PathBuf,
    report_dir: PathBuf,
    profile_filter: Vec<String>,
    embedding_filter: Vec<String>,
    suites: SuiteSelection,
    limit: Option<usize>,
    discover: bool,
    /// Which runtime the fixtures are measured against. A gate verdict
    /// belongs to a (model, device, runtime) triple, so this is a choice, not
    /// an assumption.
    runtime: Option<String>,
    base_url: Option<String>,
    /// Deployed servable name, when it differs from the profile's model tag.
    servable: Option<String>,
    sequence_report: Option<PathBuf>,
    superpowers_root: Option<PathBuf>,
    sequence_repetitions: u32,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cortex-eval: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), EvalError> {
    let options = parse_args()?;
    let raw = std::fs::read_to_string(&options.config)
        .map_err(|error| EvalError::Config(format!("{}: {error}", options.config.display())))?;
    let config: EvalConfigFile =
        serde_json::from_str(&raw).map_err(|error| EvalError::Config(error.to_string()))?;
    if config.profiles.is_empty() {
        return Err(EvalError::Config("no profiles configured".to_owned()));
    }

    let mut ollama = OllamaConfig::default();
    if let Some(base_url) = &config.base_url {
        ollama.base_url.clone_from(base_url);
    }
    let timeout =
        std::time::Duration::from_secs(config.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS).max(1));
    ollama.request_timeout = timeout;
    ollama.read_timeout = timeout;
    ollama.write_timeout = timeout;
    for profile in &config.profiles {
        ollama = ollama.with_profile(
            profile.id.clone(),
            ModelProfile::new(
                profile.model.clone(),
                profile.max_input_tokens,
                profile.max_output_tokens,
                profile.context_tokens,
            ),
        );
    }
    for profile in &config.embedding_profiles {
        ollama = ollama.with_profile(
            profile.id.clone(),
            ModelProfile::new(
                profile.model.clone(),
                profile.max_input_tokens,
                1,
                profile.max_input_tokens.saturating_add(1),
            ),
        );
    }
    let backend = open_backend(ollama, timeout, &config, &options)?;
    let backend = backend.as_ref();

    if options.discover {
        discover(backend, &config.profiles);
        return Ok(());
    }

    let fixtures = default_fixtures()?;
    let selected: Vec<EvalProfile> = config
        .profiles
        .iter()
        .filter(|profile| {
            options.profile_filter.is_empty() || options.profile_filter.contains(&profile.id)
        })
        .map(|profile| EvalProfile {
            id: profile.id.clone(),
            tier: profile.tier,
            model: profile.model.clone(),
        })
        .collect();
    if selected.is_empty() {
        return Err(EvalError::Config(
            "profile filter matched no configured profile".to_owned(),
        ));
    }

    let chat_selected =
        options.suites.classification || options.suites.extraction || options.suites.compression;
    let profiles = if chat_selected {
        selected
            .iter()
            .map(|profile| run_profile(backend, profile, &fixtures, options.suites, options.limit))
            .collect()
    } else {
        Vec::new()
    };
    let embeddings = run_embeddings(backend, &config, &options, &fixtures);
    let sequences = run_sequences(backend, &selected, &options)?;
    let report = EvalReport {
        generated_at_unix: unix_now(),
        ollama_version: backend.version().ok(),
        prompt_version: PROMPT_VERSION.to_owned(),
        schema_version: SCHEMA_VERSION.to_owned(),
        profiles,
        embeddings,
        sequences,
    };
    let path = write_json(&options.report_dir, &report)?;
    print!("{}", render_markdown(&report));
    println!("\nreport: {}", path.display());
    Ok(())
}

/// Build the backend the fixtures will be measured against.
///
/// A gate verdict belongs to a (model, device, runtime) triple, so the runtime
/// is a first-class choice rather than an assumption baked into the harness.
/// `--base-url` overrides the config file so the same fixtures can be pointed
/// at an accelerator endpoint without editing anything.
fn open_backend(
    mut ollama: OllamaConfig,
    timeout: std::time::Duration,
    config: &EvalConfigFile,
    options: &CliOptions,
) -> Result<Box<dyn EvalBackend>, EvalError> {
    if let Some(base_url) = &options.base_url {
        ollama.base_url.clone_from(base_url);
    }
    match options.runtime.as_deref().unwrap_or("ollama") {
        "ollama" => {
            let client =
                OllamaClient::new(ollama).map_err(|error| EvalError::Config(error.to_string()))?;
            Ok(Box::new(OllamaEvalBackend::new(client)))
        }
        "openai" | "openai_compatible" => {
            let mut backend =
                OpenAiEvalBackend::new(&ollama.base_url, timeout, options.servable.clone())
                    .map_err(EvalError::Config)?;
            // The runner addresses profiles by id; the runtime knows models.
            for profile in &config.profiles {
                backend = backend.with_profile(&profile.id, &profile.model);
            }
            for profile in &config.embedding_profiles {
                backend = backend.with_profile(&profile.id, &profile.model);
            }
            Ok(Box::new(backend))
        }
        other => Err(EvalError::Config(format!(
            "unknown runtime {other}; expected ollama or openai"
        ))),
    }
}

fn run_embeddings(
    backend: &dyn EvalBackend,
    config: &EvalConfigFile,
    options: &CliOptions,
    fixtures: &cortex_eval::fixtures::FixtureSet,
) -> Vec<cortex_eval::runner::EmbeddingReport> {
    if !options.suites.retrieval {
        return Vec::new();
    }
    config
        .embedding_profiles
        .iter()
        .filter(|profile| {
            options.embedding_filter.is_empty() || options.embedding_filter.contains(&profile.id)
        })
        .map(|profile| {
            run_embedding_profile(
                backend,
                &EmbeddingProfile {
                    id: profile.id.clone(),
                    model: profile.model.clone(),
                },
                &fixtures.retrieval,
                options.limit,
            )
        })
        .collect()
}

fn run_sequences(
    backend: &dyn EvalBackend,
    selected: &[EvalProfile],
    options: &CliOptions,
) -> Result<Vec<cortex_eval::sequence_suite::SequenceLiveReport>, EvalError> {
    if !options.suites.sequence {
        return Ok(Vec::new());
    }
    let deterministic_report = options.sequence_report.as_deref().ok_or_else(|| {
        EvalError::Config("--suite sequence requires --sequence-report".to_owned())
    })?;
    selected
        .iter()
        .map(|profile| {
            run_sequence_suite(
                backend,
                &SequenceLiveOptions {
                    profile_id: &profile.id,
                    model: &profile.model,
                    runtime: options.runtime.as_deref().unwrap_or("ollama"),
                    deterministic_report,
                    superpowers_root: options.superpowers_root.as_deref(),
                    repetitions: options.sequence_repetitions,
                    limit: options.limit,
                },
            )
        })
        .collect()
}

fn discover(backend: &dyn EvalBackend, profiles: &[ProfileConfig]) {
    match backend.version() {
        Ok(version) => println!("ollama version: {version}"),
        Err(error) => println!("ollama unreachable: {error}"),
    }
    let installed = backend.installed_models().unwrap_or_default();
    println!("installed models: {}", installed.len());
    for model in &installed {
        println!(
            "- {} ({} MiB, digest {})",
            model.model,
            model.size / 1_048_576,
            model.digest.get(..12).unwrap_or(&model.digest)
        );
    }
    for profile in profiles {
        let present = installed
            .iter()
            .any(|model| model.model == profile.model || model.name == profile.model);
        println!(
            "profile {} -> {}: {}",
            profile.id,
            profile.model,
            if present { "present" } else { "absent" }
        );
    }
}

fn parse_args() -> Result<CliOptions, EvalError> {
    let mut options = CliOptions {
        config: PathBuf::from("config/eval-profiles.json"),
        report_dir: PathBuf::from(".cortex-loom/eval"),
        profile_filter: Vec::new(),
        embedding_filter: Vec::new(),
        suites: SuiteSelection::all(),
        limit: None,
        discover: false,
        runtime: None,
        base_url: None,
        servable: None,
        sequence_report: None,
        superpowers_root: None,
        sequence_repetitions: 3,
    };
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--config" => options.config = PathBuf::from(required(&mut args, "--config")?),
            "--report-dir" => {
                options.report_dir = PathBuf::from(required(&mut args, "--report-dir")?);
            }
            "--profile" => options
                .profile_filter
                .push(required(&mut args, "--profile")?),
            "--embedding-profile" => options
                .embedding_filter
                .push(required(&mut args, "--embedding-profile")?),
            "--suite" => options.suites = parse_suites(&required(&mut args, "--suite")?)?,
            "--limit" => {
                let value = required(&mut args, "--limit")?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| EvalError::Config(format!("invalid --limit value {value}")))?;
                options.limit = Some(parsed.max(1));
            }
            "--runtime" => options.runtime = Some(required(&mut args, "--runtime")?),
            "--base-url" => options.base_url = Some(required(&mut args, "--base-url")?),
            "--servable" => options.servable = Some(required(&mut args, "--servable")?),
            "--sequence-report" => {
                options.sequence_report =
                    Some(PathBuf::from(required(&mut args, "--sequence-report")?));
            }
            "--superpowers-root" => {
                options.superpowers_root =
                    Some(PathBuf::from(required(&mut args, "--superpowers-root")?));
            }
            "--sequence-repetitions" => {
                let value = required(&mut args, "--sequence-repetitions")?;
                options.sequence_repetitions = value.parse().map_err(|_| {
                    EvalError::Config(format!("invalid --sequence-repetitions value {value}"))
                })?;
            }
            "--discover" => options.discover = true,
            other => return Err(EvalError::Config(format!("unknown argument {other}"))),
        }
    }
    Ok(options)
}

fn required(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, EvalError> {
    args.next()
        .ok_or_else(|| EvalError::Config(format!("{flag} requires a value")))
}

fn parse_suites(value: &str) -> Result<SuiteSelection, EvalError> {
    let mut selection = SuiteSelection {
        classification: false,
        extraction: false,
        compression: false,
        retrieval: false,
        sequence: false,
    };
    for part in value.split(',') {
        match part.trim() {
            "all" => selection = SuiteSelection::all(),
            "classification" => selection.classification = true,
            "extraction" => selection.extraction = true,
            "compression" => selection.compression = true,
            "retrieval" => selection.retrieval = true,
            "sequence" => selection.sequence = true,
            other => return Err(EvalError::Config(format!("unknown suite {other}"))),
        }
    }
    if !(selection.classification
        || selection.extraction
        || selection.compression
        || selection.retrieval
        || selection.sequence)
    {
        return Err(EvalError::Config("no suite selected".to_owned()));
    }
    Ok(selection)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
