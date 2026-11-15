//! The registry's permission settings (§15A): `allow_commands` entries are validated —
//! one command prefix each, no shell or pattern metacharacters, never a catch-all — and
//! `allow_flag` must name where the command goes.

use orch_config::agents;

fn reg(user: &str) -> Result<agents::Registry, agents::AgentsError> {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("agents.toml");
    std::fs::write(&p, user).unwrap();
    agents::registry(agents::DEFAULT_TOML, &p)
}

#[test]
fn a_valid_allow_commands_list_is_accepted() {
    let r = reg("[claude]\nallow_commands = [\"cargo build\", \"cargo test --workspace\"]\n").unwrap();
    assert_eq!(
        r.get("claude").unwrap().allow_commands,
        ["cargo build", "cargo test --workspace"]
    );
}

#[test]
fn the_defaults_allow_nothing_and_claude_has_the_allow_flag() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("agents.toml");
    let r = agents::registry(agents::DEFAULT_TOML, &p).unwrap();
    for (kind, ak) in r.0.iter() {
        assert!(ak.allow_commands.is_empty(), "{kind} ships allow_commands");
        if kind == "claude" {
            assert_eq!(ak.allow_flag.as_deref(), Some("--allowedTools Bash({cmd}:*)"));
        }
    }
}

#[test]
fn bad_entries_are_rejected_with_a_clear_message() {
    let cases: &[(&str, &str)] = &[
        ("", "an empty entry is not a command"),
        ("padded ", "leading or trailing spaces"),
        ("*", "would allow every command"),
        ("cargo test; rm -rf /", "contains ';'"),
        ("cargo | tee", "contains '|'"),
        ("$(whoami)", "contains '$'"),
        ("back`tick", "contains '`'"),
        ("-rf", "looks like an option"),
    ];
    for (entry, want) in cases {
        let toml = format!("[claude]\nallow_commands = [{entry:?}]\n");
        let e = reg(&toml).unwrap_err();
        assert!(
            e.to_string().contains(want),
            "entry {entry:?}: {e} should mention {want:?}"
        );
        assert!(e.to_string().contains("allow_commands"), "{e}");
    }
}

#[test]
fn an_allow_flag_without_cmd_is_rejected() {
    let e = reg("[claude]\nallow_flag = \"--allowedTools\"\n").unwrap_err();
    assert!(
        e.to_string().contains("must contain {cmd} where the command goes"),
        "{e}"
    );
}
