//! Finding things and reading what `orch serve` prints. No orchestration logic lives here:
//! the shell starts the server, finds its token and hands both over.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Page origins the server must accept besides its own: the window's origin. (In a debug
/// build also the Vite dev server the window loads.)
pub fn allowed_origins() -> Vec<&'static str> {
    let mut origins = vec!["tauri://localhost", "http://tauri.localhost"];
    if cfg!(debug_assertions) {
        origins.extend(["http://127.0.0.1:5173", "http://localhost:5173"]);
    }
    origins
}

pub(crate) fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Find `name`: the path in `setting` (the value of `env_name`) when it is set, else the first
/// executable called `name` on `path`. The error says what to do.
pub fn find_binary(
    name: &str,
    env_name: &str,
    setting: Option<OsString>,
    path: Option<OsString>,
) -> Result<PathBuf, String> {
    if let Some(value) = setting.filter(|v| !v.is_empty()) {
        let p = PathBuf::from(&value);
        return if is_executable_file(&p) {
            Ok(p)
        } else {
            Err(format!(
                "{env_name} is set to {}, which is not an executable file: fix it or unset it",
                p.display()
            ))
        };
    }
    path.iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(name))
        .find(|p| is_executable_file(p))
        .ok_or_else(|| {
            format!("{name} was not found: set {env_name} to its full path or put it on your PATH")
        })
}

/// `find_binary` with the real environment.
pub fn find_in_environment(name: &str, env_name: &str) -> Result<PathBuf, String> {
    find_binary(
        name,
        env_name,
        std::env::var_os(env_name),
        std::env::var_os("PATH"),
    )
}

/// `orch serve: listening on http://127.0.0.1:43211` -> `http://127.0.0.1:43211`.
pub fn parse_listening(line: &str) -> Option<String> {
    let url = line.strip_prefix("orch serve: listening on ")?.trim();
    url.starts_with("http://").then(|| url.to_string())
}

/// `token: read it from /home/me/.config/herdr-orchestra/api-token and send …` -> that path.
pub fn parse_token_path(line: &str) -> Option<PathBuf> {
    let rest = line.strip_prefix("token: read it from ")?;
    let end = rest.rfind(" and send")?;
    let path = &rest[..end];
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// The token from the file `orch serve` named.
pub fn read_token(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read the API token file {}: {e}", path.display()))?;
    let token = text.trim();
    if token.is_empty() {
        return Err(format!("the API token file {} is empty", path.display()));
    }
    Ok(token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tool(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("orch-gui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn setting_wins_over_the_path() {
        let d = scratch("setting");
        let mine = tool(&d, "my-orch", 0o755);
        let other = tool(&d, "orch", 0o755);
        let found = find_binary(
            "orch",
            "ORCH_BIN",
            Some(mine.clone().into()),
            Some(d.clone().into()),
        );
        assert_eq!(found, Ok(mine));
        let _ = other;
    }

    #[test]
    fn a_bad_setting_is_an_error_not_a_silent_fallback() {
        let d = scratch("bad");
        tool(&d, "orch", 0o755);
        let e = find_binary(
            "orch",
            "ORCH_BIN",
            Some("/nonexistent/orch".into()),
            Some(d.into()),
        )
        .unwrap_err();
        assert!(
            e.contains("ORCH_BIN") && e.contains("/nonexistent/orch"),
            "{e}"
        );
    }

    #[test]
    fn searches_the_path_for_executables_only() {
        let d = scratch("path");
        tool(&d, "orch", 0o644); // not executable
        let e = find_binary("orch", "ORCH_BIN", None, Some(d.clone().into())).unwrap_err();
        assert!(
            e.contains("not found") || e.contains("was not found"),
            "{e}"
        );
        assert!(e.contains("ORCH_BIN") && e.contains("PATH"), "{e}");
        let ok = tool(&d, "orch", 0o755);
        assert_eq!(
            find_binary("orch", "ORCH_BIN", None, Some(d.into())),
            Ok(ok)
        );
        assert!(find_binary("orch", "ORCH_BIN", None, None).is_err());
    }

    #[test]
    fn an_empty_setting_counts_as_unset() {
        let d = scratch("empty");
        let ok = tool(&d, "herdr", 0o755);
        let found = find_binary("herdr", "HERDR_BIN", Some("".into()), Some(d.into()));
        assert_eq!(found, Ok(ok));
    }

    #[test]
    fn reads_what_orch_serve_prints() {
        assert_eq!(
            parse_listening("orch serve: listening on http://127.0.0.1:43211"),
            Some("http://127.0.0.1:43211".into())
        );
        assert_eq!(parse_listening("project: /x"), None);
        assert_eq!(parse_listening("orch serve: listening on ftp://x"), None);
        assert_eq!(
            parse_token_path(
                "token: read it from /home/me/.config/herdr-orchestra/api-token and send `Authorization: Bearer <token>`"
            ),
            Some(PathBuf::from("/home/me/.config/herdr-orchestra/api-token"))
        );
        assert_eq!(
            parse_token_path("token: read it from /a b/c and send `x`"),
            Some(PathBuf::from("/a b/c"))
        );
        assert_eq!(parse_token_path("token: nothing"), None);
    }

    #[test]
    fn reads_the_token_file() {
        let d = scratch("token");
        let f = d.join("api-token");
        std::fs::write(&f, "abc123\n").unwrap();
        assert_eq!(read_token(&f), Ok("abc123".into()));
        std::fs::write(&f, "  \n").unwrap();
        assert!(read_token(&f).unwrap_err().contains("empty"));
        assert!(
            read_token(&d.join("missing"))
                .unwrap_err()
                .contains("cannot read")
        );
    }

    #[test]
    fn allows_the_window_origin_and_nothing_remote() {
        let o = allowed_origins();
        assert!(o.contains(&"tauri://localhost"));
        assert!(o.iter().all(|x| !x.contains('*')));
        assert!(o.iter().all(|x| x.starts_with("tauri://")
            || x.contains("localhost")
            || x.contains("127.0.0.1")));
    }
}
