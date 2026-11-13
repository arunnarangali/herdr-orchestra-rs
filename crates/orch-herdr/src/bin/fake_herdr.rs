//! `fake-herdr`: a stand-in for the `herdr` executable (0.9.5) that the test
//! suite drives through real subprocess calls, so no test needs a real herdr.
//!
//! State lives in the directory named by `FAKE_HERDR_DIR`:
//!
//! - `agents/<name>.json`: one agent, in herdr's JSON shape (`agent`,
//!   `agent_status`, `pane_id`, `tab_id`, `workspace_id`, ...);
//! - `panes/<pane id>.json`: one pane (`pane_id`, `workspace_id`, `tab_id`,
//!   `label`, `cwd`, `agent`, `agent_status`, ...);
//! - `workspaces.json`: an array of workspaces;
//! - `read/<name>.txt`: what `agent read` prints for that agent;
//! - `current_pane`: the id `pane current` answers with (absent: no current pane);
//! - `sim.json`: simulations, all optional: `slow_ms` (sleep before answering),
//!   `fail_exit` (exit non-zero with `fail_message` on stderr, no JSON),
//!   `fail_cmd` (limit both to commands starting with this text, e.g. `"agent prompt"`);
//! - `kinds.txt`: the `kinds:` line of plain `herdr agent` (default `pi|claude|codex|gemini|agy|opencode|amp|hermes`);
//! - written by the fake: `calls.jsonl` (every invocation's arguments),
//!   `sent.jsonl` (every prompt, text, key press and command received),
//!   `pids.log` (its process id on every start).
//!
//! Not found is natural (no such agent or pane file); a prompt to an agent
//! whose `agent_status` is `blocked` is refused with `agent_blocked`, as the
//! real herdr does.

use serde_json::{Map, Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

type Reply = Result<Value, (String, String)>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Ok(dir) = std::env::var("FAKE_HERDR_DIR") else {
        eprintln!("fake-herdr: FAKE_HERDR_DIR is not set");
        std::process::exit(2);
    };
    let dir = PathBuf::from(dir);
    let _ = fs::create_dir_all(&dir);
    append(&dir, "pids.log", &format!("{}\n", std::process::id()));
    append(&dir, "calls.jsonl", &format!("{}\n", json!({ "args": args })));

    let sim: Value = read_json(&dir.join("sim.json")).unwrap_or(Value::Null);
    let line = args.join(" ");
    let applies = sim.get("fail_cmd").and_then(Value::as_str).is_none_or(|p| line.starts_with(p));
    if applies {
        if let Some(ms) = sim.get("slow_ms").and_then(Value::as_u64) {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        if let Some(code) = sim.get("fail_exit").and_then(Value::as_i64) {
            let msg = sim.get("fail_message").and_then(Value::as_str).unwrap_or("fake herdr failure");
            eprintln!("{msg}");
            std::process::exit(code as i32);
        }
    }

    if args == ["agent"] {
        // Like herdr 0.9.5: usage on stderr, exit 2, ending in the kinds line.
        let kinds = fs::read_to_string(dir.join("kinds.txt")).unwrap_or_default();
        let kinds = kinds.trim();
        let kinds = if kinds.is_empty() { "pi|claude|codex|gemini|agy|opencode|amp|hermes" } else { kinds };
        eprintln!("usage:\n  herdr agent list\n  herdr agent get <target>\n  kinds: {kinds}");
        std::process::exit(2);
    }

    let id = format!("cli:{}", args.iter().take(2).cloned().collect::<Vec<_>>().join(":"));
    match dispatch(&dir, &args) {
        Ok(Value::String(raw)) => print!("{raw}"),
        Ok(result) => println!("{}", json!({ "id": id, "result": result })),
        Err((code, message)) => {
            println!("{}", json!({ "id": id, "error": { "code": code, "message": message } }));
            std::process::exit(1);
        }
    }
}

fn dispatch(dir: &Path, args: &[String]) -> Reply {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["status"] => Ok(Value::String("server:\n  status: running\n".into())),
        ["agent", "list"] => {
            let mut agents = load_dir(&dir.join("agents"));
            agents.sort_by_key(|v| str_of(v, "name"));
            Ok(json!({ "type": "agent_list", "agents": agents }))
        }
        ["agent", "get", t] => Ok(json!({ "type": "agent_info", "agent": find_agent(dir, t)?.1 })),
        ["agent", "prompt", t, text] => {
            let (_, agent) = find_agent(dir, t)?;
            if str_of(&agent, "agent_status") == "blocked" {
                return Err(("agent_blocked".into(), format!("agent {t} is blocked at an approval prompt")));
            }
            record(dir, json!({ "kind": "prompt", "target": t, "text": text }));
            Ok(json!({ "type": "ok" }))
        }
        ["agent", "read", t, rest @ ..] => {
            let (_, agent) = find_agent(dir, t)?;
            let lines = flag(rest, "--lines").and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
            let name = str_of(&agent, "name");
            let text = fs::read_to_string(dir.join("read").join(format!("{name}.txt"))).unwrap_or_default();
            let all: Vec<&str> = text.lines().collect();
            let tail = &all[all.len().saturating_sub(lines)..];
            Ok(Value::String(tail.iter().map(|l| format!("{l}\n")).collect()))
        }
        ["agent", "rename", t, name] => {
            let (path, mut agent) = find_agent(dir, t)?;
            agent["name"] = json!(name);
            let _ = fs::remove_file(path);
            write_json(&dir.join("agents").join(format!("{name}.json")), &agent);
            Ok(json!({ "type": "agent_info", "agent": agent }))
        }
        ["pane", "current", ..] => {
            let id = fs::read_to_string(dir.join("current_pane")).unwrap_or_default();
            let id = id.trim();
            if id.is_empty() {
                return Err(("pane_not_found".into(), "no current pane".into()));
            }
            Ok(json!({ "type": "pane_current", "pane": find_pane(dir, id)?.1 }))
        }
        ["pane", "get", id] => Ok(json!({ "type": "pane_info", "pane": find_pane(dir, id)?.1 })),
        ["pane", "list", rest @ ..] => {
            let mut panes = load_dir(&dir.join("panes"));
            if let Some(w) = flag(rest, "--workspace") {
                panes.retain(|p| str_of(p, "workspace_id") == w);
            }
            panes.sort_by_key(|v| str_of(v, "pane_id"));
            Ok(json!({ "type": "pane_list", "panes": panes }))
        }
        ["pane", "rename", id, label @ ..] => {
            let (path, mut pane) = find_pane(dir, id)?;
            pane["label"] = json!(label.join(" "));
            write_json(&path, &pane);
            Ok(json!({ "type": "pane_info", "pane": pane }))
        }
        ["pane", "split", rest @ ..] => split(dir, rest),
        ["pane", "run", id, command @ ..] => {
            find_pane(dir, id)?;
            record(dir, json!({ "kind": "run", "target": id, "text": command.join(" ") }));
            Ok(json!({ "type": "ok" }))
        }
        ["pane", "send-text", id, text] => {
            find_pane(dir, id)?;
            record(dir, json!({ "kind": "text", "target": id, "text": text }));
            Ok(json!({ "type": "ok" }))
        }
        ["pane", "send-keys", id, keys @ ..] if !keys.is_empty() => {
            find_pane(dir, id)?;
            record(dir, json!({ "kind": "keys", "target": id, "keys": keys }));
            Ok(json!({ "type": "ok" }))
        }
        ["workspace", "list"] => {
            let ws = read_json(&dir.join("workspaces.json")).unwrap_or_else(|| json!([]));
            Ok(json!({ "type": "workspace_list", "workspaces": ws }))
        }
        _ => Err(("invalid_request".into(), format!("fake-herdr: unsupported command: {}", args.join(" ")))),
    }
}

fn split(dir: &Path, rest: &[&str]) -> Reply {
    let source = rest.first().filter(|s| !s.starts_with("--")).map(|s| s.to_string());
    let source = match source {
        Some(s) => s,
        None => fs::read_to_string(dir.join("current_pane")).unwrap_or_default().trim().to_string(),
    };
    let (_, parent) = find_pane(dir, &source)?;
    let counter = dir.join("next_pane");
    let n: u64 = fs::read_to_string(&counter).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(100);
    let _ = fs::write(&counter, (n + 1).to_string());
    let ws = str_of(&parent, "workspace_id");
    let id = format!("{ws}:p{n}");
    let mut pane = Map::new();
    pane.insert("pane_id".into(), json!(id));
    pane.insert("workspace_id".into(), json!(ws));
    pane.insert("tab_id".into(), parent["tab_id"].clone());
    pane.insert("agent_status".into(), json!("unknown"));
    pane.insert("focused".into(), json!(rest.contains(&"--focus")));
    pane.insert("cwd".into(), flag(rest, "--cwd").map_or(parent["cwd"].clone(), |c| json!(c)));
    let pane = Value::Object(pane);
    write_json(&dir.join("panes").join(format!("{id}.json")), &pane);
    record(dir, json!({ "kind": "split", "target": source, "direction": flag(rest, "--direction"), "cwd": flag(rest, "--cwd") }));
    Ok(json!({ "type": "pane_info", "pane": pane }))
}

fn flag<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.iter().position(|a| *a == name).and_then(|i| args.get(i + 1)).copied()
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// An agent by name or pane id: its file and its JSON.
fn find_agent(dir: &Path, target: &str) -> Result<(PathBuf, Value), (String, String)> {
    let found = fs::read_dir(dir.join("agents"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| read_json(&e.path()).map(|v| (e.path(), v)))
        .find(|(_, v)| str_of(v, "name") == target || str_of(v, "pane_id") == target);
    found.ok_or_else(|| ("agent_not_found".into(), format!("agent target {target} not found")))
}

fn find_pane(dir: &Path, id: &str) -> Result<(PathBuf, Value), (String, String)> {
    let path = dir.join("panes").join(format!("{id}.json"));
    read_json(&path).map(|v| (path, v)).ok_or_else(|| ("pane_not_found".into(), format!("pane {id} not found")))
}

fn load_dir(d: &Path) -> Vec<Value> {
    fs::read_dir(d).into_iter().flatten().flatten().filter_map(|e| read_json(&e.path())).collect()
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(p).ok()?).ok()
}

fn write_json(p: &Path, v: &Value) {
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(p, serde_json::to_string_pretty(v).unwrap_or_default());
}

fn record(dir: &Path, entry: Value) {
    append(dir, "sent.jsonl", &format!("{entry}\n"));
}

fn append(dir: &Path, file: &str, text: &str) {
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join(file)) {
        let _ = f.write_all(text.as_bytes());
    }
}
