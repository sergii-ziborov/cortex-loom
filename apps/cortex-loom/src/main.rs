use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cortex_adapters::{AgentKind, McpLaunch, export_product_adapter};
use cortex_mcp::{AgentPacket, AgentPrepare, CortexMcpState, expand_saved, prepare_packet};
use serde_json::{Value, json};

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cortex-loom: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: &[String]) -> Result<(), String> {
    let Some(command) = arguments.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };
    match command {
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        "-V" | "--version" | "version" => {
            println!("cortex-loom {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "doctor" => doctor(&arguments[1..]),
        "prepare" => prepare(&arguments[1..]),
        "expand" => expand(&arguments[1..]),
        "setup" => setup(&arguments[1..]),
        "report" => report(&arguments[1..]),
        other => Err(format!("unknown command: {other}. Try cortex-loom --help.")),
    }
}

fn print_help() {
    println!(
        "cortex-loom {} - local prepare/expand over the same compiler as cortex-mcp\n\n\
         Commands:\n  \
         doctor [--repo <path>]\n  \
         prepare --repo <path> (--task <text> | --task-file <file> | --task-stdin) [--budget 6000] [--format json]\n  \
         expand --packet <id> --facet <facet> [--repo <path>]\n  \
         setup --agent claude-code|codex|copilot [--dry-run]\n  \
         report --last\n\n\
         Use exactly one task source. JSON goes to stdout; diagnostics to stderr.\n\
         Setup is preview-only. Studio UI is not this binary.",
        env!("CARGO_PKG_VERSION")
    );
}

fn doctor(arguments: &[String]) -> Result<(), String> {
    let flags = if arguments.is_empty() {
        Flags::default()
    } else {
        Flags::parse(arguments)?
    };
    let repository = flags
        .optional("repo")
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let database = database_in(&repository);
    let weavatrix = match CortexMcpState::open(database.clone()) {
        Ok(_) => "ok".to_owned(),
        Err(error) => format!("error: {error}"),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "product": "cortex-loom",
            "version": env!("CARGO_PKG_VERSION"),
            "mcp": "cortex-mcp --profile agent",
            "repository": repository,
            "database": database,
            "weavatrix": weavatrix,
            "online": "off - local compile only",
            "modelRequired": false,
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn prepare(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::parse(arguments)?;
    let repository = flags
        .path("repo")?
        .canonicalize()
        .map_err(|error| format!("--repo: {error}. Pass an existing repository."))?;
    flags.require_json_format()?;
    let task = flags.task()?;
    let budget = flags.optional("budget");
    let budget_class = budget.as_deref().map(budget_class_from_tokens);
    let state = CortexMcpState::open(database_in(&repository))?;
    let packet = prepare_packet(
        &state,
        AgentPrepare {
            repository: repository.clone(),
            task: task.clone(),
            run_id: flags.optional("run-id"),
            budget_class,
            classifier_model: flags.optional("classifier-model"),
        },
    )?;
    persist_last(&repository, &task, &packet)?;
    print_json(&packet)?;
    Ok(())
}

fn expand(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::parse(arguments)?;
    flags.require_json_format()?;
    let packet_id = flags.value("packet")?;
    let facet = flags.value("facet")?;
    let repository = flags
        .optional("repo")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .canonicalize()
        .map_err(|error| format!("--repo: {error}"))?;
    let stored = load_packet(&repository, &packet_id)?;
    let state = CortexMcpState::open(database_in(&repository))?;
    print_json(&expand_saved(&state, stored, &facet)?)
}

fn setup(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::parse(arguments)?;
    let raw = flags.value("agent")?;
    let agent = AgentKind::parse(&raw)
        .or_else(|| AgentKind::parse(&raw.replace('-', "_")))
        .ok_or_else(|| "unknown --agent. Use claude-code, codex, or copilot.".to_owned())?;
    let launch = McpLaunch {
        command: "cortex-mcp".to_owned(),
        args: vec!["--profile".to_owned(), "agent".to_owned()],
    };
    let bundle = export_product_adapter(agent, &launch);
    print_json(&serde_json::to_value(&bundle).map_err(|error| error.to_string())?)?;
    if flags.has("write") {
        return Err("refusing --write: setup stays preview-only.".to_owned());
    }
    eprintln!("preview only; nothing was written.");
    Ok(())
}

fn report(arguments: &[String]) -> Result<(), String> {
    if !arguments.iter().any(|argument| argument == "--last") {
        return Err("report needs --last".to_owned());
    }
    let repository = PathBuf::from(".")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let path = cli_dir(&repository).join("last.json");
    let text = fs::read_to_string(&path).map_err(|_| {
        format!(
            "no last packet at {}. Run cortex-loom prepare first.",
            path.display()
        )
    })?;
    print!("{text}");
    if !text.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn persist_last(repository: &Path, task: &str, packet: &Value) -> Result<(), String> {
    let dir = cli_dir(repository);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let id = packet
        .get("packetId")
        .and_then(Value::as_str)
        .ok_or_else(|| "prepare returned no packetId".to_owned())?;
    let record = json!({
        "task": task,
        "packet": packet,
    });
    let encoded = serde_json::to_string_pretty(&record).map_err(|error| error.to_string())?;
    fs::write(dir.join("last.json"), &encoded).map_err(|error| error.to_string())?;
    fs::write(dir.join(format!("{id}.json")), encoded).map_err(|error| error.to_string())
}

fn load_packet(repository: &Path, packet_id: &str) -> Result<AgentPacket, String> {
    let path = cli_dir(repository).join(format!("{packet_id}.json"));
    let fallback = cli_dir(repository).join("last.json");
    let text = fs::read_to_string(&path)
        .or_else(|_| fs::read_to_string(&fallback))
        .map_err(|_| {
            format!("unknown packetId: {packet_id}. Run cortex-loom prepare in this repo.")
        })?;
    let root: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let packet = root.get("packet").cloned().unwrap_or_else(|| root.clone());
    if packet.get("packetId").and_then(Value::as_str) != Some(packet_id) && path.exists() {
        return Err(format!("stored file is not {packet_id}"));
    }
    Ok(AgentPacket {
        id: packet_id.to_owned(),
        repository: packet
            .get("repository")
            .and_then(Value::as_str)
            .map_or_else(|| repository.to_path_buf(), PathBuf::from),
        task: root
            .get("task")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        run_id: None,
        task_hash: packet
            .get("taskHash")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        symbols: packet
            .get("symbols")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        snapshot_id: packet
            .get("snapshotId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        certificate_hash: packet
            .get("certificateHash")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        max_tokens: packet
            .get("maxTokens")
            .and_then(Value::as_u64)
            .and_then(|tokens| u32::try_from(tokens).ok())
            .unwrap_or(4000),
    })
}

fn budget_class_from_tokens(value: &str) -> String {
    match value.parse::<u32>().unwrap_or(4000) {
        0..=2500 => "tight".to_owned(),
        2501..=8000 => "normal".to_owned(),
        _ => "wide".to_owned(),
    }
}

fn print_json(value: &Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn cli_dir(repository: &Path) -> PathBuf {
    repository.join(".cortex-loom").join("cli")
}

fn database_in(repository: &Path) -> PathBuf {
    repository.join(".cortex-loom").join("cortex-loom.db")
}

#[derive(Default)]
struct Flags {
    pairs: Vec<(String, String)>,
    switches: Vec<String>,
}

impl Flags {
    fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut pairs = Vec::new();
        let mut switches = Vec::new();
        let mut index = 0;
        while index < arguments.len() {
            let raw = arguments[index].as_str();
            if !raw.starts_with("--") {
                return Err(format!("unexpected argument: {raw}"));
            }
            let name = raw.trim_start_matches('-').to_owned();
            if matches!(name.as_str(), "dry-run" | "write" | "last" | "task-stdin") {
                switches.push(name);
                index += 1;
                continue;
            }
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| format!("--{name} requires a value"))?
                .clone();
            pairs.push((name, value));
            index += 2;
        }
        Ok(Self { pairs, switches })
    }

    fn value(&self, name: &str) -> Result<String, String> {
        self.optional(name)
            .ok_or_else(|| format!("missing --{name}"))
    }

    fn optional(&self, name: &str) -> Option<String> {
        self.pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }

    fn path(&self, name: &str) -> Result<PathBuf, String> {
        Ok(PathBuf::from(self.value(name)?))
    }

    fn has(&self, name: &str) -> bool {
        self.switches.iter().any(|item| item == name)
    }

    fn require_json_format(&self) -> Result<(), String> {
        match self.optional("format").as_deref() {
            None | Some("json") => Ok(()),
            Some(other) => Err(format!(
                "unsupported --format {other}. This release prints JSON only."
            )),
        }
    }

    fn task(&self) -> Result<String, String> {
        let inline = self.optional("task");
        let file = self.optional("task-file");
        let stdin = self.has("task-stdin");
        let sources =
            usize::from(inline.is_some()) + usize::from(file.is_some()) + usize::from(stdin);
        if sources > 1 {
            return Err("use only one of --task, --task-file, --task-stdin".to_owned());
        }
        if let Some(text) = inline {
            return Ok(text);
        }
        if let Some(path) = file {
            return fs::read_to_string(path).map_err(|error| error.to_string());
        }
        if stdin {
            use std::io::Read;
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|error| error.to_string())?;
            return Ok(text);
        }
        Err("missing --task, --task-file, or --task-stdin".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_pins_follow_token_request() {
        assert_eq!(budget_class_from_tokens("2000"), "tight");
        assert_eq!(budget_class_from_tokens("6000"), "normal");
        assert_eq!(budget_class_from_tokens("16000"), "wide");
    }

    #[test]
    fn flags_parse_dry_run_and_repo() {
        let flags = Flags::parse(&[
            "--agent".to_owned(),
            "claude-code".to_owned(),
            "--dry-run".to_owned(),
        ])
        .unwrap();
        assert_eq!(flags.value("agent").unwrap(), "claude-code");
        assert!(flags.has("dry-run"));
    }

    #[test]
    fn flags_reject_two_task_sources() {
        let flags = Flags::parse(&[
            "--task".to_owned(),
            "Who calls prepare?".to_owned(),
            "--task-file".to_owned(),
            "task.md".to_owned(),
        ])
        .unwrap();
        assert!(flags.task().unwrap_err().contains("only one"));
    }
}
