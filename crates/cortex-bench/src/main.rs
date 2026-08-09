//! `cortex-bench` — run the three-arm context benchmark.
//!
//! ```text
//! cargo run -p cortex-bench -- [--repo PATH] [--budget N] [--task ID]
//!                              [--out PATH] [--no-weavatrix] [--stamp TEXT]
//! ```
//!
//! Nothing is written outside `--out` and nothing is sent anywhere. The
//! Weavatrix arms build a native graph over `--repo`, which is the slow part;
//! `--no-weavatrix` skips them and still measures the naive arm.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cortex_bench::naive::{NaiveScan, scan};
use cortex_bench::probe_tasks::probe_tasks;
use cortex_bench::report::render;
use cortex_bench::tasks::{BenchTask, find, tasks};
use cortex_bench::{
    ArmKind, ArmMeasurement, BenchReport, DEFAULT_BUDGET, TaskResult, measure, measure_scoped,
    unavailable,
};
use cortex_weavatrix::plan::PlanPolicy;
use cortex_weavatrix::{
    EvidenceBundle, WeavatrixAdapter, WeavatrixConfig, compile_evidence_bundle,
};

fn main() -> ExitCode {
    let settings = match Settings::from_args() {
        Ok(settings) => settings,
        Err(message) => {
            eprintln!("cortex-bench: {message}");
            return ExitCode::FAILURE;
        }
    };
    match run(&settings) {
        Ok(report) => {
            print!("{}", render(&report));
            match write_json(&settings.out, &report) {
                Ok(()) => println!("JSON report: {}", settings.out.display()),
                Err(error) => {
                    eprintln!("cortex-bench: could not write the report: {error}");
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("cortex-bench: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(settings: &Settings) -> Result<BenchReport, String> {
    let selected: Vec<&BenchTask> = match (&settings.task, settings.set.as_str()) {
        (Some(id), _) => vec![find_any(id).ok_or_else(|| format!("unknown task: {id}"))?],
        (None, "probe") => probe_tasks().iter().collect(),
        (None, "core") => tasks().iter().collect(),
        (None, "all") => tasks().iter().chain(probe_tasks().iter()).collect(),
        (None, other) => return Err(format!("unknown --set value: {other} (core|probe|all)")),
    };
    let weavatrix = if settings.use_weavatrix {
        Some(WeavatrixAdapter::new(
            WeavatrixConfig::discover().map_err(|error| error.to_string())?,
        ))
    } else {
        None
    };
    let mut results = Vec::with_capacity(selected.len());
    for task in selected {
        eprintln!("cortex-bench: {}", task.id);
        results.push(run_task(settings, task, weavatrix.as_ref()));
    }
    Ok(BenchReport {
        repository: settings.repository.display().to_string(),
        budget: settings.budget,
        stamp: settings.stamp.clone(),
        tasks: results,
    })
}

fn run_task(
    settings: &Settings,
    task: &BenchTask,
    weavatrix: Option<&WeavatrixAdapter>,
) -> TaskResult {
    let mut arms = vec![naive_arm(settings, task)];
    match weavatrix {
        None => {
            let reason = "skipped: --no-weavatrix";
            arms.push(unavailable(ArmKind::WeavatrixRaw, reason));
            arms.push(unavailable(ArmKind::CortexLoom, reason));
        }
        Some(adapter) => {
            match prepare(adapter, &settings.repository, task) {
                Ok(bundle) => {
                    arms.push(weavatrix_raw_arm(task, &bundle));
                    arms.push(cortex_arm(ArmKind::CortexLoom, settings, task, bundle));
                }
                Err(error) => {
                    arms.push(unavailable(ArmKind::WeavatrixRaw, error.clone()));
                    arms.push(unavailable(ArmKind::CortexLoom, error));
                }
            }
            match adapter.prepare_targeted_context(
                &settings.repository,
                task.prompt,
                task.symbol,
                settings.budget,
            ) {
                Ok(bundle) => {
                    // Same evidence twice: once as Weavatrix returned it,
                    // once through the compiler. Any difference is the
                    // compiler's, and nothing else can be credited to it.
                    arms.push(planned_raw_arm(task, &bundle));
                    arms.push(cortex_arm(
                        ArmKind::CortexLoomTargeted,
                        settings,
                        task,
                        bundle,
                    ));
                }
                Err(error) => {
                    let reason = error.to_string();
                    arms.push(unavailable(ArmKind::WeavatrixPlanned, reason.clone()));
                    arms.push(unavailable(ArmKind::CortexLoomTargeted, reason));
                }
            }
            // Nothing trimmed: the operations the budget normally drops are
            // fetched anyway, so the trimming itself can be scored.
            let untrimmed = PlanPolicy {
                overcommit: 1_000,
                ..PlanPolicy::default()
            };
            match adapter.prepare_targeted_context_with(
                &settings.repository,
                task.prompt,
                task.symbol,
                settings.budget,
                untrimmed,
            ) {
                Ok(bundle) => {
                    arms.push(cortex_arm(ArmKind::CortexLoomFull, settings, task, bundle));
                }
                Err(error) => arms.push(unavailable(ArmKind::CortexLoomFull, error.to_string())),
            }
            // Search hits name files; open bounded windows there and ask
            // whether the remaining contract/transport facts appear without
            // paying the naive whole-file cost.
            match adapter.prepare_verified_targeted_context(
                &settings.repository,
                task.prompt,
                task.symbol,
                settings.budget,
                PlanPolicy::default(),
                cortex_weavatrix::PlanHints::default(),
            ) {
                Ok((mut bundle, report)) => {
                    bundle.warnings.push(format!(
                        "sufficiency: {}; retry: {}",
                        report.sufficient, report.retry_performed
                    ));
                    arms.push(cortex_arm(
                        ArmKind::CortexLoomSource,
                        settings,
                        task,
                        bundle,
                    ));
                }
                Err(error) => {
                    arms.push(unavailable(ArmKind::CortexLoomSource, error.to_string()));
                }
            }
        }
    }
    TaskResult {
        task_id: task.id.to_owned(),
        prompt: task.prompt.to_owned(),
        budget: settings.budget,
        anchor_count: task.anchors.len(),
        arms,
    }
}

fn naive_arm(settings: &Settings, task: &BenchTask) -> ArmMeasurement {
    match scan(&settings.repository, task.naive_globs) {
        Ok(found) => finish_naive(task, &found),
        Err(error) => unavailable(ArmKind::Naive, error.to_string()),
    }
}

fn finish_naive(task: &BenchTask, found: &NaiveScan) -> ArmMeasurement {
    let mut arm = measure(
        ArmKind::Naive,
        &found.context(),
        found.files.len(),
        task.anchors,
    );
    if found.files.is_empty() {
        arm.notes
            .push(format!("no file matched {:?}", task.naive_globs));
    }
    for skipped in &found.skipped {
        arm.notes.push(format!("skipped {skipped}"));
    }
    arm
}

/// Ask Weavatrix for evidence, falling back to a symbol-free bundle when the
/// symbol is unknown to the graph — an absent symbol is a property of the
/// repository, not a harness failure, and it belongs in the notes.
fn prepare(
    adapter: &WeavatrixAdapter,
    repository: &Path,
    task: &BenchTask,
) -> Result<EvidenceBundle, String> {
    match adapter.prepare_context(repository, task.prompt, task.symbol) {
        Ok(bundle) => Ok(bundle),
        Err(error) if task.symbol.is_some() => {
            let first = error.to_string();
            adapter
                .prepare_context(repository, task.prompt, None)
                .map(|mut bundle| {
                    bundle
                        .warnings
                        .push(format!("symbol evidence unavailable: {first}"));
                    bundle
                })
                .map_err(|second| format!("{first}; without symbol: {second}"))
        }
        Err(error) => Err(error.to_string()),
    }
}

fn weavatrix_raw_arm(task: &BenchTask, bundle: &EvidenceBundle) -> ArmMeasurement {
    let mut arm = measure(
        ArmKind::WeavatrixRaw,
        &concatenate(bundle),
        bundle.evidence.len(),
        task.anchors,
    );
    arm.notes.push("no budget applied".to_owned());
    for warning in &bundle.warnings {
        arm.notes.push(warning.clone());
    }
    arm
}

/// The planned operations exactly as Weavatrix returned them: already
/// token-budgeted by the tool, but with no ordering, no trust states, and no
/// cross-operation view.
fn planned_raw_arm(task: &BenchTask, bundle: &EvidenceBundle) -> ArmMeasurement {
    let mut arm = measure(
        ArmKind::WeavatrixPlanned,
        &concatenate(bundle),
        bundle.evidence.len(),
        task.anchors,
    );
    arm.notes
        .push("Weavatrix token_budget only, no compiler".to_owned());
    arm
}

/// Same rendering the compiler uses, so arms differ only in selection,
/// ordering, and deduplication — never in framing.
fn concatenate(bundle: &EvidenceBundle) -> String {
    let mut context = String::new();
    for fragment in &bundle.evidence {
        let _ = write!(
            context,
            "## [{}] {}\n{}\n\n",
            fragment.id, fragment.source, fragment.content
        );
    }
    context
}

fn cortex_arm(
    arm_kind: ArmKind,
    settings: &Settings,
    task: &BenchTask,
    bundle: EvidenceBundle,
) -> ArmMeasurement {
    match compile_evidence_bundle(bundle, task.prompt, settings.budget, None) {
        Ok(compiled) => {
            let packet = &compiled.context;
            let mut arm = measure_scoped(
                arm_kind,
                &packet.content,
                &without_task_echo(&packet.content),
                packet.included_ids.len(),
                task.anchors,
            );
            if !packet.omitted_ids.is_empty() {
                arm.notes.push(format!(
                    "omitted under budget: {}",
                    packet.omitted_ids.join(", ")
                ));
            }
            if packet.deduplicated_lines > 0 {
                arm.notes.push(format!(
                    "deduplicated {} repeated lines (~{} tokens)",
                    packet.deduplicated_lines, packet.deduplicated_estimated_tokens
                ));
            }
            if packet.requires_upstream {
                arm.notes.push("packet requires upstream review".to_owned());
            }
            arm.notes.extend(compiled.warnings.iter().cloned());
            arm
        }
        // Fail-closed is a result, not a crash: a budget too small for
        // critical evidence must be visible in the report.
        Err(error) => unavailable(arm_kind, format!("fail-closed: {error}")),
    }
}

/// Drop the synthetic `TASK` section the compiler prepends.
///
/// It is sent, so it counts towards tokens, but it is the question and not
/// the evidence: scoring it would let a prompt that names a symbol satisfy
/// that anchor without anything having been retrieved.
fn without_task_echo(packet: &str) -> String {
    packet
        .split("\n## ")
        .filter(|section| {
            // The first section keeps its own `## ` prefix; later ones lost
            // theirs to the separator.
            !section
                .trim_start()
                .trim_start_matches("## ")
                .starts_with("[TASK]")
        })
        .collect::<Vec<_>>()
        .join("\n## ")
}

fn write_json(path: &Path, report: &BenchReport) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(report)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    std::fs::write(path, body)
}

struct Settings {
    repository: PathBuf,
    budget: u32,
    task: Option<String>,
    set: String,
    out: PathBuf,
    use_weavatrix: bool,
    stamp: Option<String>,
}

impl Settings {
    fn from_args() -> Result<Self, String> {
        let mut settings = Self {
            repository: PathBuf::from("."),
            budget: DEFAULT_BUDGET,
            task: None,
            set: "core".to_owned(),
            out: Path::new(".cortex-loom").join("bench").join("report.json"),
            use_weavatrix: true,
            stamp: None,
        };
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--repo" => settings.repository = PathBuf::from(next(&mut arguments, "--repo")?),
                "--budget" => {
                    settings.budget = next(&mut arguments, "--budget")?
                        .parse()
                        .map_err(|_| "--budget expects a positive integer".to_owned())?;
                }
                "--task" => settings.task = Some(next(&mut arguments, "--task")?),
                "--set" => settings.set = next(&mut arguments, "--set")?,
                "--out" => settings.out = PathBuf::from(next(&mut arguments, "--out")?),
                "--stamp" => settings.stamp = Some(next(&mut arguments, "--stamp")?),
                "--no-weavatrix" => settings.use_weavatrix = false,
                "--list" => {
                    for task in tasks() {
                        println!("{}", task.id);
                    }
                    for task in probe_tasks() {
                        println!("{}", task.id);
                    }
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        if settings.budget == 0 {
            return Err("--budget must be greater than zero".to_owned());
        }
        Ok(settings)
    }
}

fn find_any(id: &str) -> Option<&'static BenchTask> {
    find(id).or_else(|| probe_tasks().iter().find(|task| task.id == id))
}

fn next(arguments: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))
}
