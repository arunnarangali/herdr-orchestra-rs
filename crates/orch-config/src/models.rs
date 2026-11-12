//! The model catalog: shipped `models.default.toml` merged with the user's `models.toml`
//! (or `ORCH_MODELS_FILE`) — policy and tiers merged per key, models per id, as Python's
//! `models_catalog()` does. Deny patterns are shell-style globs matched case-insensitively.

use globset::{Glob, GlobSet, GlobSetBuilder};
use orch_core::error::EXIT_USAGE;
use orch_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The shipped catalog, embedded in the binary (v4.10.1, unchanged).
pub const DEFAULT_TOML: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/models.default.toml"));

/// Tier names, weakest first.
pub const TIERS: [&str; 3] = ["small", "medium", "high"];
/// The seven 1–5 ratings a model carries.
pub const RATINGS: [&str; 7] = [
    "reasoning",
    "coding",
    "architecture",
    "instructions",
    "tools",
    "planning",
    "speed",
];

#[derive(Debug, thiserror::Error)]
#[error("model catalog: {file}: {problem}")]
pub struct ModelsError {
    /// File the problem is in (`models.default.toml` or the override path).
    pub file: String,
    /// What is wrong with it.
    pub problem: String,
}

impl From<ModelsError> for Error {
    fn from(e: ModelsError) -> Self {
        Error::wrap(e, EXIT_USAGE)
    }
}

/// `[policy]`: models nobody in a run may use.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Policy {
    /// Shell-style patterns matched case-insensitively against the model id.
    #[serde(default)]
    pub deny: Vec<String>,
    /// Why they are denied, shown by `orch models`.
    #[serde(default)]
    pub deny_why: Option<String>,
}

/// One tier: the reasoning rating a model needs to serve it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tier {
    /// The floor: coding skill alone never makes a model `high`.
    #[serde(default)]
    pub reasoning: i64,
    /// What the tier is for, shown by `orch models`.
    #[serde(rename = "use", default)]
    pub use_: Option<String>,
}

/// `[tiers]`: small / medium / high.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tiers {
    #[serde(default)]
    pub small: Tier,
    #[serde(default)]
    pub medium: Tier,
    #[serde(default)]
    pub high: Tier,
}

impl Tiers {
    /// The tier by name, for the cap-down walk.
    pub fn get(&self, tier: &str) -> Option<&Tier> {
        match tier {
            "small" => Some(&self.small),
            "medium" => Some(&self.medium),
            "high" => Some(&self.high),
            _ => None,
        }
    }
}

/// One model in the catalog.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Model {
    /// Agent kinds that can run the model.
    #[serde(default)]
    pub kinds: Vec<String>,
    /// small | medium | high — what the model is trusted with.
    #[serde(default)]
    pub tier: Option<String>,
    /// Multi-step problem solving, debugging without a hint.
    #[serde(default)]
    pub reasoning: Option<i64>,
    /// Writing and fixing code that passes its checks.
    #[serde(default)]
    pub coding: Option<i64>,
    /// Design across modules, contracts between parts.
    #[serde(default)]
    pub architecture: Option<i64>,
    /// Follows the task file, scope and output format reliably.
    #[serde(default)]
    pub instructions: Option<i64>,
    /// Tool calling and long agentic sessions.
    #[serde(default)]
    pub tools: Option<i64>,
    /// Breaking a goal into steps and keeping to them.
    #[serde(default)]
    pub planning: Option<i64>,
    /// 5 = fastest and cheapest, 1 = slowest and most expensive.
    #[serde(default)]
    pub speed: Option<i64>,
    /// Context window in thousands of tokens.
    #[serde(default)]
    pub context_k: Option<i64>,
    /// True when the weights can be downloaded and self-hosted.
    #[serde(default)]
    pub open_weights: Option<bool>,
    /// Where the ratings come from, with a date.
    #[serde(default)]
    pub evidence: Option<String>,
}

impl From<toml::de::Error> for ModelsError {
    fn from(e: toml::de::Error) -> Self {
        ModelsError {
            file: "models.default.toml".to_string(),
            problem: e.to_string(),
        }
    }
}

/// The whole catalog: model id (lowercased) -> its entry.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub policy: Policy,
    pub tiers: Tiers,
    pub models: BTreeMap<String, Model>,
    deny_set: GlobSet,
}

/// The user's override file: `ORCH_MODELS_FILE` when set (and not empty), else
/// `~/.config/herdr-orchestra/models.toml`, as Python's `user_models_file()` does.
pub fn user_models_file() -> PathBuf {
    match std::env::var_os("ORCH_MODELS_FILE") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home_dir()
            .join(".config")
            .join("herdr-orchestra")
            .join("models.toml"),
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Shipped defaults merged with `user_file` (missing file is fine, a broken one is an
/// error naming it). `None` uses [`user_models_file`].
pub fn load(user_file: Option<&Path>) -> Result<Catalog> {
    let path = user_file.map_or_else(user_models_file, Path::to_path_buf);
    catalog(DEFAULT_TOML, &path).map_err(Error::from)
}

/// Merge a default catalog text with one user override file. `[policy]` and `[tiers]`
/// merge per key; each model merges per key; model ids are lowercased.
pub fn catalog(default_toml: &str, user_file: &Path) -> std::result::Result<Catalog, ModelsError> {
    let mut root = parse(default_toml, "models.default.toml")?;
    if user_file.exists() {
        let text = read(user_file)?;
        let user = parse(&text, &file_name(user_file))?;
        for sec in ["policy", "tiers"] {
            if let Some(vals) = user.get(sec).and_then(|v| v.as_table()) {
                let dst = root
                    .get(sec)
                    .and_then(|v| v.as_table().cloned())
                    .unwrap_or_default();
                let dst = root
                    .entry(sec.to_string())
                    .or_insert(toml::Value::Table(dst));
                if let Some(t) = dst.as_table_mut() {
                    for (k, v) in vals {
                        t.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        if let Some(models) = user.get("models").and_then(|v| v.as_table()) {
            let dst = root
                .get("models")
                .and_then(|v| v.as_table().cloned())
                .unwrap_or_default();
            let dst = root
                .entry("models".to_string())
                .or_insert(toml::Value::Table(dst));
            if let Some(t) = dst.as_table_mut() {
                for (mid, vals) in models {
                    if let Some(vals) = vals.as_table() {
                        let id = mid.to_lowercase();
                        let mut merged = t
                            .get(&id)
                            .and_then(|v| v.as_table().cloned())
                            .unwrap_or_default();
                        for (k, v) in vals {
                            merged.insert(k.clone(), v.clone());
                        }
                        t.insert(id, toml::Value::Table(merged));
                    }
                }
            }
        }
    }
    let policy: Policy = take_table(&mut root, "policy")
        .transpose()?
        .unwrap_or_default();
    let tiers: Tiers = take_table(&mut root, "tiers")
        .transpose()?
        .unwrap_or_default();
    let mut models = BTreeMap::new();
    if let Some(table) = root.get("models").and_then(|v| v.as_table()) {
        for (mid, vals) in table {
            let m: Model = Model::deserialize(vals.clone()).map_err(|e| ModelsError {
                file: file_name(user_file),
                problem: format!("model {mid:?}: {e}"),
            })?;
            models.insert(mid.clone(), m);
        }
    }
    let mut builder = GlobSetBuilder::new();
    for pat in &policy.deny {
        let glob = Glob::new(&pat.to_lowercase()).map_err(|e| ModelsError {
            file: file_name(user_file),
            problem: format!("deny pattern {pat:?}: {e}"),
        })?;
        builder.add(glob);
    }
    let deny_set = builder.build().map_err(|e| ModelsError {
        file: file_name(user_file),
        problem: format!("deny patterns: {e}"),
    })?;
    Ok(Catalog {
        policy,
        tiers,
        models,
        deny_set,
    })
}

fn take_table<T: serde::de::DeserializeOwned>(
    root: &mut toml::Table,
    key: &str,
) -> Option<std::result::Result<T, toml::de::Error>> {
    root.remove(key).map(|v| v.try_into())
}

/// The policy pattern that forbids this model, or None (`model_denied`).
pub fn denied<'a>(policy: &'a Policy, deny_set: &GlobSet, mid: &str) -> Option<&'a String> {
    let mid = mid.to_lowercase();
    if mid.is_empty() {
        return None;
    }
    deny_set
        .matches(Path::new(mid.as_str()))
        .into_iter()
        .next()
        .map(|i| &policy.deny[i])
}

impl Catalog {
    /// The policy pattern that forbids this model, or None (`model_denied`).
    pub fn denied(&self, mid: &str) -> Option<&String> {
        denied(&self.policy, &self.deny_set, mid)
    }

    /// The tier a model serves: its `tier`, stepped down when its reasoning is below the
    /// floor of that tier (`model_tier`). None when it is not in the catalog or its tier
    /// is not one of [`TIERS`].
    pub fn model_tier(&self, mid: &str) -> Option<&'static str> {
        let m = self.models.get(&mid.to_lowercase())?;
        let mut tier = m.tier.as_deref()?.to_lowercase();
        self.tiers.get(&tier)?;
        if let Some(r) = m.reasoning {
            while let Some(i) = TIERS.iter().position(|t| *t == tier) {
                if i == 0 || r >= self.tiers.get(&tier).map(|t| t.reasoning).unwrap_or(0) {
                    break;
                }
                tier = TIERS[i - 1].to_owned();
            }
        }
        Some(tier_str(&tier))
    }

    /// Models whose reasoning rating is below their tier's floor (reported as invalid;
    /// they still serve the tier below, see [`Catalog::model_tier`]).
    pub fn invalid_models(&self) -> Vec<String> {
        self.models
            .iter()
            .filter(|(_, m)| {
                let Some(tier) = m.tier.as_deref().map(str::to_lowercase) else {
                    return false;
                };
                let Some(t) = self.tiers.get(&tier) else {
                    return false;
                };
                m.reasoning.is_some_and(|r| r < t.reasoning)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }
}

fn tier_str(tier: &str) -> &'static str {
    TIERS
        .iter()
        .find(|t| **t == tier)
        .expect("tier was validated")
}

fn parse(text: &str, file: &str) -> std::result::Result<toml::Table, ModelsError> {
    text.parse::<toml::Table>()
        .map_err(|e: toml::de::Error| ModelsError {
            file: file.to_string(),
            problem: e.to_string(),
        })
}

fn read(path: &Path) -> std::result::Result<String, ModelsError> {
    std::fs::read_to_string(path).map_err(|e| ModelsError {
        file: file_name(path),
        problem: format!("cannot read it: {e}"),
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> PathBuf {
        PathBuf::from("/nonexistent/models.toml")
    }

    fn user(t: &tempfile::TempDir, toml_text: &str) -> PathBuf {
        let p = t.path().join("models.toml");
        std::fs::write(&p, toml_text).unwrap();
        p
    }

    #[test]
    fn shipped_defaults_parse_with_every_model() {
        let cat = catalog(DEFAULT_TOML, &none()).unwrap();
        for mid in [
            "haiku",
            "sonnet",
            "opus",
            "gpt-6-luna",
            "gpt-6-sol",
            "qwen3.6-35b-a3b",
            "qwen3.8-27b",
            "deepseek-v4-pro",
            "glm-5.3",
        ] {
            assert!(cat.models.contains_key(mid), "missing {mid}");
        }
        assert_eq!(cat.models.len(), 9);
    }

    #[test]
    fn shipped_model_keys_are_todays() {
        let cat = catalog(DEFAULT_TOML, &none()).unwrap();
        let m = cat.models.get("sonnet").unwrap();
        assert_eq!(m.kinds, ["claude"]);
        assert_eq!(m.tier.as_deref(), Some("medium"));
        assert_eq!(m.reasoning, Some(4));
        assert_eq!(m.coding, Some(4));
        assert_eq!(m.architecture, Some(3));
        assert_eq!(m.instructions, Some(5));
        assert_eq!(m.tools, Some(5));
        assert_eq!(m.planning, Some(4));
        assert_eq!(m.speed, Some(3));
        assert_eq!(m.context_k, Some(200));
        assert_eq!(m.open_weights, Some(false));
        assert!(m.evidence.as_deref().unwrap().contains("2026-11"));
        assert_eq!(
            cat.models.get("qwen3.6-35b-a3b").unwrap().open_weights,
            Some(true)
        );
    }

    #[test]
    fn shipped_policy_and_tiers() {
        let cat = catalog(DEFAULT_TOML, &none()).unwrap();
        assert_eq!(cat.policy.deny, ["*fable*", "best", "*astra*"]);
        assert_eq!(cat.tiers.small.reasoning, 1);
        assert_eq!(cat.tiers.medium.reasoning, 3);
        assert_eq!(cat.tiers.high.reasoning, 4);
        assert!(cat.tiers.high.use_.as_deref().unwrap().contains("lead"));
    }

    #[test]
    fn deny_patterns_match_case_insensitive_globs() {
        let cat = catalog(DEFAULT_TOML, &none()).unwrap();
        assert_eq!(
            cat.denied("claude-fable-5-2").map(String::as_str),
            Some("*fable*")
        );
        assert_eq!(cat.denied("BEST").map(String::as_str), Some("best"));
        assert_eq!(
            cat.denied("gemini-astra-9").map(String::as_str),
            Some("*astra*")
        );
        assert_eq!(cat.denied("sonnet"), None);
        assert_eq!(cat.denied("opus"), None);
        assert_eq!(cat.denied(""), None);
    }

    #[test]
    fn model_tier_serves_its_tier() {
        let cat = catalog(DEFAULT_TOML, &none()).unwrap();
        assert_eq!(cat.model_tier("haiku"), Some("small"));
        assert_eq!(cat.model_tier("sonnet"), Some("medium"));
        assert_eq!(cat.model_tier("opus"), Some("high"));
        assert_eq!(cat.model_tier("gpt-6-luna"), Some("small"));
        assert_eq!(cat.model_tier("glm-5.3"), Some("high"));
        assert_eq!(cat.model_tier("unknown-model"), None);
        assert_eq!(cat.model_tier("SONNET"), Some("medium"));
    }

    #[test]
    fn reasoning_below_the_tier_floor_is_invalid_and_caps_down() {
        let t = tempfile::tempdir().unwrap();
        let p = user(
            &t,
            "[models.wannabe-high]\nkinds = [\"claude\"]\ntier = \"high\"\nreasoning = 3\n",
        );
        let cat = catalog(DEFAULT_TOML, &p).unwrap();
        assert_eq!(cat.invalid_models(), ["wannabe-high"]);
        assert_eq!(cat.model_tier("wannabe-high"), Some("medium"));
        assert!(
            catalog(DEFAULT_TOML, &none())
                .unwrap()
                .invalid_models()
                .is_empty()
        );
    }

    #[test]
    fn override_one_key_of_one_model() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[models.sonnet]\nreasoning = 5\n");
        let cat = catalog(DEFAULT_TOML, &p).unwrap();
        let sonnet = cat.models.get("sonnet").unwrap();
        assert_eq!(sonnet.reasoning, Some(5));
        assert_eq!(sonnet.coding, Some(4));
        assert_eq!(sonnet.kinds, ["claude"]);
        assert_eq!(cat.models.len(), 9);
    }

    #[test]
    fn add_a_new_model() {
        let t = tempfile::tempdir().unwrap();
        let p = user(
            &t,
            "[models.my-model]\nkinds = [\"acme\"]\ntier = \"small\"\nreasoning = 2\n",
        );
        let cat = catalog(DEFAULT_TOML, &p).unwrap();
        assert_eq!(
            cat.models.get("my-model").unwrap().tier.as_deref(),
            Some("small")
        );
        assert_eq!(cat.models.len(), 10);
    }

    #[test]
    fn change_the_deny_list() {
        let t = tempfile::tempdir().unwrap();
        let p = user(
            &t,
            "[policy]\ndeny = [\"*secret*\"]\ndeny_why = \"our own rule\"\n",
        );
        let cat = catalog(DEFAULT_TOML, &p).unwrap();
        assert_eq!(cat.policy.deny, ["*secret*"]);
        assert_eq!(cat.denied("claude-fable-5-2"), None);
        assert_eq!(
            cat.denied("my-secret-thing").map(String::as_str),
            Some("*secret*")
        );
        assert_eq!(cat.policy.deny_why.as_deref(), Some("our own rule"));
    }

    #[test]
    fn override_a_tier_floor_per_key() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[tiers.high]\nreasoning = 5\n");
        let cat = catalog(DEFAULT_TOML, &p).unwrap();
        assert_eq!(cat.tiers.high.reasoning, 5);
        assert_eq!(cat.tiers.medium.reasoning, 3);
        assert!(cat.tiers.high.use_.is_none());
        assert_eq!(cat.model_tier("opus"), Some("high"));
        assert_eq!(cat.invalid_models(), ["deepseek-v4-pro", "glm-5.3"]);
    }

    #[test]
    fn broken_toml_names_the_file() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[models.sonnet\nreasoning = 5\n");
        let e = catalog(DEFAULT_TOML, &p).unwrap_err();
        assert_eq!(e.file, "models.toml");
        assert!(e.to_string().contains("model catalog: models.toml:"), "{e}");
    }

    #[test]
    fn wrong_type_names_the_model_and_the_problem() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[models.sonnet]\nreasoning = \"four\"\n");
        let e = catalog(DEFAULT_TOML, &p).unwrap_err();
        assert_eq!(e.file, "models.toml");
        assert!(e.problem.contains("sonnet"), "{e}");
    }

    #[test]
    fn invalid_deny_pattern_is_an_error_naming_the_file() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[policy]\ndeny = [\"[a\"]\n");
        let e = catalog(DEFAULT_TOML, &p).unwrap_err();
        assert_eq!(e.file, "models.toml");
        assert!(e.problem.contains("[a"), "{e}");
    }

    #[test]
    fn missing_override_file_is_fine() {
        let cat = catalog(DEFAULT_TOML, &none()).unwrap();
        assert_eq!(cat.models.len(), 9);
    }

    #[test]
    fn load_uses_the_embedded_defaults() {
        let cat = load(Some(&none())).unwrap();
        assert_eq!(cat.model_tier("sonnet"), Some("medium"));
        assert_eq!(
            cat.denied("claude-fable-5-2").map(String::as_str),
            Some("*fable*")
        );
    }

    #[test]
    fn user_models_file_follows_the_env_var() {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = LOCK.lock().unwrap();
        unsafe { std::env::set_var("ORCH_MODELS_FILE", "/tmp/from-env/models.toml") };
        assert_eq!(
            user_models_file(),
            PathBuf::from("/tmp/from-env/models.toml")
        );
        unsafe { std::env::remove_var("ORCH_MODELS_FILE") };
        assert!(user_models_file().ends_with("herdr-orchestra/models.toml"));
        unsafe { std::env::set_var("ORCH_MODELS_FILE", "") };
        assert!(user_models_file().ends_with("herdr-orchestra/models.toml"));
        unsafe { std::env::remove_var("ORCH_MODELS_FILE") };
    }
}
