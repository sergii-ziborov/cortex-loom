use super::*;

#[test]
fn exact_budget_rejects_bad_values_before_compiling() {
    for value in ["0", "abc", "100001"] {
        let error = run(&[
            "prepare".to_owned(),
            "--repo".to_owned(),
            ".".to_owned(),
            "--task".to_owned(),
            "task".to_owned(),
            "--budget".to_owned(),
            value.to_owned(),
        ])
        .unwrap_err();
        assert!(error.contains("--max-tokens"), "{error}");
    }
}

#[test]
fn typo_flag_is_rejected() {
    let flags = Flags::parse(&["--llm-bakend".to_owned(), "off".to_owned()]).unwrap();
    assert!(flags.validate(&["llm-backend"], &[]).is_err());
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

#[test]
fn unknown_command_points_at_help() {
    let error = run(&["serve".to_owned()]).unwrap_err();
    assert!(error.contains("unknown command"));
    assert!(error.contains("--help"));
}

#[test]
fn prepare_rejects_non_json_format() {
    let error = run(&[
        "prepare".to_owned(),
        "--repo".to_owned(),
        ".".to_owned(),
        "--task".to_owned(),
        "Who calls prepare?".to_owned(),
        "--format".to_owned(),
        "markdown".to_owned(),
    ])
    .unwrap_err();
    assert!(error.contains("unsupported --format"));
}

#[test]
fn setup_refuses_write() {
    let error = run(&[
        "setup".to_owned(),
        "--agent".to_owned(),
        "claude-code".to_owned(),
        "--write".to_owned(),
    ])
    .unwrap_err();
    assert!(error.contains("preview-only"));
}

#[test]
fn report_requires_last_flag() {
    assert!(run(&["report".to_owned()]).unwrap_err().contains("--last"));
}

#[test]
fn flags_require_a_task_source() {
    let flags = Flags::parse(&["--repo".to_owned(), ".".to_owned()]).unwrap();
    assert!(flags.task().unwrap_err().contains("missing --task"));
}

#[test]
fn load_packet_reads_last_json() {
    let root = std::env::temp_dir().join(format!("cortex-loom-cli-{}", std::process::id()));
    let dir = cli_dir(&root);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
            dir.join("last.json"),
            r#"{"task":"Who calls prepare?","packet":{"packetId":"pk_test","taskHash":"abc","symbols":["prepare"],"maxTokens":4000}}"#,
        )
        .unwrap();
    let packet = load_packet(&root, "pk_test").unwrap();
    assert_eq!(packet.id, "pk_test");
    assert_eq!(packet.task, "Who calls prepare?");
    assert_eq!(packet.symbols, ["prepare"]);
    let _ = fs::remove_dir_all(&root);
}
