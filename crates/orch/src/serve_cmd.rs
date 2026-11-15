//! `orch serve [--host <loopback address>] [--port <n>] [--allow-origin <origin>]…`: the local
//! REST API (`orch-api`). It listens on loopback only, answers only requests with the
//! install's token, and only serves the run that exists: it never creates or edits one.
//! `--allow-origin` lets one more web page origin (the desktop app's window) call it.

use crate::listing::die;
use orch_api::{AppState, BindError};
use orch_core::paths;
use std::path::PathBuf;

const USAGE: &str = "usage: orch serve [--host <127.0.0.1|localhost|::1>] [--port <n>] \
[--allow-origin <scheme://host[:port]>]…";

struct Opts {
    host: String,
    port: u16,
    origins: Vec<String>,
}

/// An origin is `scheme://host[:port]`: no path, no wildcard.
fn valid_origin(o: &str) -> bool {
    o.split_once("://").is_some_and(|(scheme, rest)| {
        !scheme.is_empty()
            && !rest.is_empty()
            && !o.contains('*')
            && !rest.contains(['/', '?', '#', ' '])
    })
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut opts = Opts {
        host: "127.0.0.1".into(),
        port: 0,
        origins: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (a.as_str(), None),
        };
        if !["--host", "--port", "--allow-origin"].contains(&flag) {
            return Err(format!("unknown argument `{a}`\n{USAGE}"));
        }
        let value = match inline.or_else(|| it.next().cloned()) {
            Some(v) => v,
            None => return Err(format!("{flag} needs a value\n{USAGE}")),
        };
        if flag == "--host" {
            opts.host = value;
        } else if flag == "--allow-origin" {
            if !valid_origin(&value) {
                return Err(format!(
                    "--allow-origin must look like http://localhost:5173 (scheme://host[:port], \
no path or wildcard), got `{value}`"
                ));
            }
            opts.origins.push(value);
        } else {
            opts.port = value
                .parse()
                .map_err(|_| format!("--port must be a number from 0 to 65535, got `{value}`"))?;
        }
    }
    Ok(opts)
}

/// The project to serve: the main repository root as seen from the current folder, or the
/// folder itself when it is not a git repository (it then has no run).
fn project_root() -> Result<PathBuf, String> {
    let cwd =
        std::env::current_dir().map_err(|e| format!("cannot read the current folder: {e}"))?;
    Ok(paths::main_root(&cwd).unwrap_or(cwd))
}

/// Resolves on the first SIGTERM or SIGINT. The handlers stay installed afterwards: a second
/// signal while the server is closing its streams and finishing requests ends the process at
/// once instead of being swallowed.
async fn wait_for_stop() {
    use tokio::signal::unix::{SignalKind, signal};
    let term = signal(SignalKind::terminate());
    let int = signal(SignalKind::interrupt());
    match (term, int) {
        (Ok(mut term), Ok(mut int)) => {
            let which = tokio::select! {
                _ = term.recv() => "SIGTERM",
                _ = int.recv() => "SIGINT",
            };
            println!("orch serve: {which} received — closing streams and stopping (send it again to exit now)");
            tokio::spawn(async move {
                tokio::select! {
                    _ = term.recv() => {}
                    _ = int.recv() => {}
                }
                eprintln!("orch serve: second signal — exiting now");
                std::process::exit(130);
            });
        }
        // Without signal handlers the only stop is the default (kill), so wait forever.
        _ => std::future::pending::<()>().await,
    }
}

pub fn serve(args: &[String]) -> i32 {
    let opts = match parse(args) {
        Ok(o) => o,
        Err(e) => return die(e),
    };
    let Some(cfg) = paths::config_dir() else {
        return die("cannot find your config folder: set HOME or XDG_CONFIG_HOME");
    };
    let root = match project_root() {
        Ok(r) => r,
        Err(e) => return die(e),
    };
    let token = match orch_api::token::load_or_create(&cfg) {
        Ok(t) => t,
        Err(e) => return die(format!("API token: {e}")),
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return die(format!("cannot start the async runtime: {e}")),
    };
    rt.block_on(async {
        let listener = match orch_api::bind(&opts.host, opts.port).await {
            Ok(l) => l,
            Err(e @ BindError::NotLoopback(_)) => return die(e),
            Err(e) => return die(format!("cannot listen on {}:{}: {e}", opts.host, opts.port)),
        };
        let addr = match listener.local_addr() {
            Ok(a) => a,
            Err(e) => return die(e),
        };
        println!("orch serve: listening on http://{addr}");
        println!("project: {}", root.display());
        println!(
            "token: read it from {} and send `Authorization: Bearer <token>`",
            orch_api::token::token_path(&cfg).display()
        );
        let state = AppState::new(root, token);
        match orch_api::serve_with(listener, state, opts.origins, wait_for_stop()).await {
            Ok(()) => {
                println!("orch serve: stopped");
                0
            }
            Err(e) => die(format!("server error: {e}")),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_flags_in_both_forms() {
        let o = parse(&v(&["--host", "localhost", "--port=8080"])).unwrap();
        assert_eq!((o.host.as_str(), o.port), ("localhost", 8080));
        let d = parse(&[]).unwrap();
        assert_eq!((d.host.as_str(), d.port), ("127.0.0.1", 0));
        assert!(d.origins.is_empty());
    }

    #[test]
    fn collects_allowed_origins() {
        let o = parse(&v(&[
            "--allow-origin",
            "tauri://localhost",
            "--allow-origin=http://localhost:5173",
        ]))
        .unwrap();
        assert_eq!(o.origins, ["tauri://localhost", "http://localhost:5173"]);
        for bad in [
            "*",
            "localhost",
            "http://x/",
            "http://*.x",
            "://x",
            "http://",
        ] {
            assert!(parse(&v(&["--allow-origin", bad])).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(&v(&["--port", "x"])).is_err());
        assert!(parse(&v(&["--port"])).is_err());
        assert!(parse(&v(&["--bogus"])).is_err());
    }
}
