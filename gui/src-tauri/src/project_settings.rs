//! Per-project settings stored in the shell's config folder (never in the project):
//! default Lead agent and the project folder path.
//! Also provides agent CLI detection across installed tools.

use crate::projects::canonical;
use crate::store::{self, Loaded};
use orch_config::agents::{Registry, Role};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const KEYS: &[&str] = &["folder", "lead_agent"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCli {
    pub kind: String,
    pub display_name: String,
    pub installed: bool,
    pub can_lead: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettings {
    pub lead_agent: Option<String>,
    pub folder: String,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProjectSettingsInput {
    pub lead_agent: Option<String>,
    pub folder: String,
}

/// A stable 128-bit FNV-1a hash of the canonical project path, formatted as a 32-char hex string.
pub fn project_hash(canonical_path: &str) -> String {
    let mut hash: u128 = 0x6c62272e07bb014262b821756295c58d;
    for byte in canonical_path.as_bytes() {
        hash ^= *byte as u128;
        hash = hash.wrapping_mul(0x1000000000000000000013b);
    }
    format!("{:032x}", hash)
}

#[cfg(unix)]
pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub fn is_executable(p: &Path) -> bool {
    p.is_file()
}

pub fn is_binary_on_path(binary: &str, path_var: Option<&OsStr>) -> bool {
    if binary.is_empty() {
        return false;
    }
    if binary.contains('/') {
        let p = Path::new(binary);
        return is_executable(p);
    }
    let Some(path_var) = path_var else {
        return false;
    };
    std::env::split_paths(path_var)
        .map(|d| d.join(binary))
        .any(|p| is_executable(&p))
}

pub fn detect_agent_clis_with(
    registry: &Registry,
    path_var: Option<&OsStr>,
) -> Vec<AgentCli> {
    registry
        .0
        .iter()
        .map(|(kind, ak)| {
            let installed = is_binary_on_path(&ak.binary, path_var);
            let can_lead = ak.roles.contains(&Role::Lead);
            let display_name = ak
                .display_name
                .clone()
                .unwrap_or_else(|| kind.clone());
            AgentCli {
                kind: kind.clone(),
                display_name,
                installed,
                can_lead,
            }
        })
        .collect()
}

pub fn validate_lead_agent(
    agent: Option<&str>,
    registry: &Registry,
    path_var: Option<&OsStr>,
) -> Result<Option<String>, String> {
    let Some(agent) = agent else {
        return Ok(None);
    };
    let trimmed = agent.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let Some(ak) = registry.get(trimmed) else {
        return Err(format!("agent `{trimmed}` is not installed"));
    };
    if !ak.roles.contains(&Role::Lead) {
        return Err(format!("agent `{trimmed}` cannot lead"));
    }
    if !is_binary_on_path(&ak.binary, path_var) {
        return Err(format!("agent `{trimmed}` is not installed"));
    }
    Ok(Some(trimmed.to_string()))
}

pub struct ProjectSettingsStore {
    dir: PathBuf,
}

impl ProjectSettingsStore {
    pub fn new(dir: PathBuf) -> Self {
        ProjectSettingsStore { dir }
    }

    pub fn file_for(&self, canonical_path: &str) -> PathBuf {
        let canon = canonical(canonical_path);
        let hash = project_hash(&canon);
        self.dir.join("project-settings").join(format!("{hash}.txt"))
    }

    pub fn load(&self, canonical_path: &str) -> Loaded<ProjectSettings> {
        let file = self.file_for(canonical_path);
        let (entries, warning) = store::read_or_set_aside(&file, KEYS);
        let mut lead_agent = None;
        let mut folder = canonical(canonical_path);
        if let Some(entries) = entries {
            let v = store::last_values(&entries);
            if let Some(la) = v.get("lead_agent") {
                let trimmed = la.trim();
                if !trimmed.is_empty() {
                    lead_agent = Some(trimmed.to_string());
                }
            }
            if let Some(f) = v.get("folder") {
                let trimmed = f.trim();
                if !trimmed.is_empty() {
                    folder = canonical(trimmed);
                }
            }
        }
        Loaded {
            value: ProjectSettings {
                lead_agent,
                folder,
                warning: warning.clone(),
            },
            warning,
        }
    }

    pub fn save(
        &self,
        canonical_path: &str,
        settings: &ProjectSettings,
    ) -> Result<ProjectSettings, String> {
        let file = self.file_for(canonical_path);
        let folder = canonical(&settings.folder);
        store::check_value(&folder)?;
        if let Some(la) = &settings.lead_agent {
            store::check_value(la)?;
        }
        let mut entries = vec![("folder", folder.as_str())];
        if let Some(la) = &settings.lead_agent {
            entries.push(("lead_agent", la.as_str()));
        }
        let text = store::render(&entries);
        store::write_atomic(&file, &text)?;
        Ok(ProjectSettings {
            lead_agent: settings.lead_agent.clone(),
            folder,
            warning: None,
        })
    }

    pub fn remove(&self, canonical_path: &str) -> Result<(), String> {
        let file = self.file_for(canonical_path);
        if file.exists() {
            let _ = std::fs::remove_file(file);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::scratch;
    use std::ffi::OsString;

    fn make_executable(path: &Path) {
        std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }
    }

    #[test]
    fn ac1_detect_agent_clis_with_fake_path() {
        let bin_dir = scratch("fake-bin");
        let claude_bin = bin_dir.join("claude");
        let codex_bin = bin_dir.join("codex");
        let custom_bin = bin_dir.join("mycustom");
        make_executable(&claude_bin);
        make_executable(&custom_bin);
        // codex is created as non-executable
        std::fs::write(&codex_bin, "not executable").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&codex_bin).unwrap().permissions();
            perms.set_mode(0o644);
            std::fs::set_permissions(&codex_bin, perms).unwrap();
        }

        // Test with user override file (agents.toml) containing a custom agent
        let user_toml = scratch("user-agents").join("agents.toml");
        std::fs::write(
            &user_toml,
            r#"
[mycustom]
binary = "mycustom"
launch = "mycustom"
roles = ["lead", "specialist"]
display_name = "My Custom Lead"
"#,
        )
        .unwrap();

        let registry =
            orch_config::agents::registry(orch_config::agents::DEFAULT_TOML, &user_toml).unwrap();

        let path_os: OsString = bin_dir.into_os_string();
        let detected = detect_agent_clis_with(&registry, Some(path_os.as_os_str()));

        // Claude is in DEFAULT_TOML with role lead
        let claude = detected.iter().find(|a| a.kind == "claude").unwrap();
        assert!(claude.installed, "executable on PATH is installed");
        assert!(claude.can_lead, "claude can lead");

        // Codex is in DEFAULT_TOML but not executable
        let codex = detected.iter().find(|a| a.kind == "codex").unwrap();
        assert!(
            !codex.installed,
            "non-executable binary on PATH is not installed"
        );
        assert!(codex.can_lead);

        // mycustom was added via user's agents.toml
        let custom = detected.iter().find(|a| a.kind == "mycustom").unwrap();
        assert_eq!(custom.display_name, "My Custom Lead");
        assert!(custom.installed);
        assert!(custom.can_lead);
    }

    #[test]
    fn ac2_and_ac3_settings_roundtrip_restart_and_lead_validation() {
        let config_dir = scratch("settings-ac2-config");
        let proj_dir = scratch("settings-ac2-proj");
        let proj_canon = canonical(&proj_dir.display().to_string());

        let bin_dir = scratch("settings-ac2-bin");
        let claude_bin = bin_dir.join("claude");
        make_executable(&claude_bin);

        let toml_str = r#"
[claude]
binary = "claude"
launch = "claude"
roles = ["lead", "coder"]
display_name = "Claude Code"

[coder_only]
binary = "coder"
launch = "coder"
roles = ["coder"]
"#;
        let registry = orch_config::agents::registry(toml_str, Path::new("/nonexistent")).unwrap();
        let path_os: OsString = bin_dir.into_os_string();
        let path_ref = Some(path_os.as_os_str());

        // Validate lead agent:
        // AC3: uninstalled agent naming the agent
        let err_uninstalled =
            validate_lead_agent(Some("missing_agent"), &registry, path_ref).unwrap_err();
        assert!(
            err_uninstalled.contains("missing_agent") && err_uninstalled.contains("not installed")
        );

        // AC3: agent that cannot lead naming the agent
        let err_not_lead =
            validate_lead_agent(Some("coder_only"), &registry, path_ref).unwrap_err();
        assert!(err_not_lead.contains("coder_only") && err_not_lead.contains("cannot lead"));

        // AC3: empty or None clears it
        assert_eq!(
            validate_lead_agent(None, &registry, path_ref).unwrap(),
            None
        );
        assert_eq!(
            validate_lead_agent(Some(""), &registry, path_ref).unwrap(),
            None
        );
        assert_eq!(
            validate_lead_agent(Some("   "), &registry, path_ref).unwrap(),
            None
        );

        // Valid lead agent
        assert_eq!(
            validate_lead_agent(Some("claude"), &registry, path_ref).unwrap(),
            Some("claude".to_string())
        );

        // AC2: save settings and verify survival across app restart (new store instance)
        let store1 = ProjectSettingsStore::new(config_dir.clone());
        let default_settings = store1.load(&proj_canon).value;
        assert_eq!(default_settings.lead_agent, None);
        assert_eq!(default_settings.folder, proj_canon);
        assert_eq!(default_settings.warning, None);

        let to_save = ProjectSettings {
            lead_agent: Some("claude".to_string()),
            folder: proj_canon.clone(),
            warning: None,
        };
        store1.save(&proj_canon, &to_save).unwrap();

        // Simulate app restart by creating a new store instance with same config dir
        let store2 = ProjectSettingsStore::new(config_dir);
        let loaded = store2.load(&proj_canon).value;
        assert_eq!(loaded.lead_agent, Some("claude".to_string()));
        assert_eq!(loaded.folder, proj_canon);
        assert_eq!(loaded.warning, None);

        // Clearing lead agent
        let cleared = ProjectSettings {
            lead_agent: None,
            folder: proj_canon.clone(),
            warning: None,
        };
        store2.save(&proj_canon, &cleared).unwrap();
        let loaded_cleared = store2.load(&proj_canon).value;
        assert_eq!(loaded_cleared.lead_agent, None);
    }

    #[test]
    fn ac5_corrupt_settings_file_set_aside_as_corrupt() {
        let config_dir = scratch("settings-ac5-config");
        let proj_dir = scratch("settings-ac5-proj");
        let proj_canon = canonical(&proj_dir.display().to_string());

        let store = ProjectSettingsStore::new(config_dir);
        let file = store.file_for(&proj_canon);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "corrupt file content with no header\n").unwrap();

        let loaded = store.load(&proj_canon);
        assert_eq!(loaded.value.lead_agent, None);
        assert_eq!(loaded.value.folder, proj_canon);
        assert!(loaded.value.warning.is_some());
        let warn = loaded.value.warning.unwrap();
        assert!(warn.contains(".corrupt"));

        // Verify the original file was set aside as .corrupt
        let corrupt_path = PathBuf::from(format!("{}.corrupt", file.display()));
        assert!(corrupt_path.exists(), "file must be moved to .corrupt");
        assert!(
            !file.exists(),
            "original corrupt file must no longer exist at original path"
        );
    }

    #[test]
    fn ac6_nothing_written_inside_project_folder() {
        let config_dir = scratch("settings-ac6-config");
        let proj_dir = scratch("settings-ac6-proj");
        std::fs::create_dir_all(proj_dir.join(".git")).unwrap();
        std::fs::write(proj_dir.join("file.txt"), "hello").unwrap();
        let proj_canon = canonical(&proj_dir.display().to_string());

        let read_entries = || -> Vec<String> {
            let mut entries: Vec<_> = std::fs::read_dir(&proj_dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            entries.sort();
            entries
        };

        let before = read_entries();

        let store = ProjectSettingsStore::new(config_dir);
        store
            .save(
                &proj_canon,
                &ProjectSettings {
                    lead_agent: Some("claude".to_string()),
                    folder: proj_canon.clone(),
                    warning: None,
                },
            )
            .unwrap();
        let _ = store.load(&proj_canon);

        let after = read_entries();
        assert_eq!(
            before, after,
            "project folder contents must remain completely unchanged"
        );
    }
}
