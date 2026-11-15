//! The application's own settings, saved in the shell's config folder (not in any project):
//! theme, desktop notifications, and the path of the `orch` binary.

use crate::serve::is_executable_file;
use crate::store::{self, Loaded};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE: &str = "settings.txt";
const KEYS: &[&str] = &["theme", "notifications", "orch_path"];
pub const THEMES: [&str; 3] = ["system", "light", "dark"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    /// `system`, `light` or `dark`.
    pub theme: String,
    pub notifications: bool,
    /// Empty: find `orch` through `ORCH_BIN` or the PATH.
    pub orch_path: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            theme: "system".into(),
            notifications: false,
            orch_path: String::new(),
        }
    }
}

impl AppSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !THEMES.contains(&self.theme.as_str()) {
            return Err(format!("the theme must be one of {}", THEMES.join(", ")));
        }
        store::check_value(&self.orch_path)?;
        if !self.orch_path.is_empty() && !is_executable_file(Path::new(&self.orch_path)) {
            return Err(format!(
                "{} is not an executable file: choose the orch binary, or leave this empty to use ORCH_BIN or the PATH",
                self.orch_path
            ));
        }
        Ok(())
    }
}

pub struct SettingsStore {
    dir: PathBuf,
}

impl SettingsStore {
    pub fn new(dir: PathBuf) -> SettingsStore {
        SettingsStore { dir }
    }

    fn file(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    pub fn load(&self) -> Loaded<AppSettings> {
        let (entries, mut warning) = store::read_or_set_aside(&self.file(), KEYS);
        let mut s = AppSettings::default();
        if let Some(entries) = entries {
            let v = store::last_values(&entries);
            if let Some(t) = v.get("theme") {
                s.theme = t.to_string();
            }
            if let Some(n) = v.get("notifications") {
                s.notifications = *n == "true";
            }
            if let Some(p) = v.get("orch_path") {
                s.orch_path = p.to_string();
            }
            // A value that no longer makes sense (the binary was deleted, a hand-edited theme)
            // falls back to the default for that field, and says so.
            if !THEMES.contains(&s.theme.as_str()) {
                s.theme = "system".into();
                warning = Some("The saved theme was not valid; using the system theme.".into());
            }
        }
        Loaded { value: s, warning }
    }

    /// Validate and save. A failed save returns the reason and changes nothing.
    pub fn save(&self, s: &AppSettings) -> Result<AppSettings, String> {
        s.validate()?;
        let notifications = if s.notifications { "true" } else { "false" };
        let text = store::render(&[
            ("theme", &s.theme),
            ("notifications", notifications),
            ("orch_path", &s.orch_path),
        ]);
        store::write_atomic(&self.file(), &text)?;
        Ok(s.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::scratch;
    use std::os::unix::fs::PermissionsExt;

    fn store(tag: &str) -> SettingsStore {
        SettingsStore::new(scratch(tag).join("config"))
    }

    fn exe(tag: &str) -> String {
        let p = scratch(tag).join("orch");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    }

    #[test]
    fn defaults_when_nothing_is_saved() {
        let l = store("defaults").load();
        assert_eq!(l.value, AppSettings::default());
        assert!(l.warning.is_none());
    }

    #[test]
    fn saved_settings_survive_a_restart() {
        let s = store("persist");
        let want = AppSettings {
            theme: "dark".into(),
            notifications: true,
            orch_path: exe("persist-bin"),
        };
        s.save(&want).unwrap();
        assert_eq!(SettingsStore::new(s.dir.clone()).load().value, want);
    }

    #[test]
    fn a_failed_save_says_why_and_keeps_what_was_saved() {
        let s = store("fail");
        let good = AppSettings {
            theme: "light".into(),
            ..AppSettings::default()
        };
        s.save(&good).unwrap();
        let e = s
            .save(&AppSettings {
                orch_path: "/no/such/orch".into(),
                ..good.clone()
            })
            .unwrap_err();
        assert!(e.contains("not an executable file"), "{e}");
        let e = s
            .save(&AppSettings {
                theme: "pink".into(),
                ..good.clone()
            })
            .unwrap_err();
        assert!(e.contains("theme"), "{e}");
        assert!(
            s.save(&AppSettings {
                orch_path: "a\nb".into(),
                ..good.clone()
            })
            .is_err()
        );
        assert_eq!(s.load().value, good);
    }

    #[test]
    fn a_corrupt_file_is_set_aside_and_defaults_are_used() {
        let s = store("corrupt");
        std::fs::create_dir_all(&s.dir).unwrap();
        std::fs::write(s.file(), "garbage").unwrap();
        let l = s.load();
        assert_eq!(l.value, AppSettings::default());
        assert!(l.warning.unwrap().contains("settings.txt.corrupt"));
        assert!(s.dir.join("settings.txt.corrupt").exists());
        s.save(&AppSettings::default()).unwrap();
        assert!(s.load().warning.is_none());
    }

    #[test]
    fn an_invalid_saved_theme_falls_back_with_a_warning() {
        let s = store("badtheme");
        std::fs::create_dir_all(&s.dir).unwrap();
        std::fs::write(s.file(), format!("{}\ntheme=pink\n", store::HEADER)).unwrap();
        let l = s.load();
        assert_eq!(l.value.theme, "system");
        assert!(l.warning.is_some());
    }
}
