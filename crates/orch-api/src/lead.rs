//! The Lead launcher: starts the project's Lead on the developer's request and says how it
//! stands, for a project with or without a run.
//!
//! Starting brings Herdr up headless when it is not running, opens a new Herdr workspace in
//! the project folder, starts the chosen agent kind there with the registry's launch command
//! (the same engine `orch team up` uses), names it after the run's lead (`lead` before there
//! is a run), labels its pane the same, and hands it the planning playbook with the
//! developer's first message as one prompt. The quick checks (the kind, Herdr) answer the
//! request; the slow part (the agent coming up) runs on its own thread, so the request never
//! waits on it, and every wait in it is bounded.
//!
//! At most one Lead: a start while one is starting or herdr already has the lead is refused
//! (`lead_running`), and the check-and-claim is one step under a lock. Nothing here can start
//! or address any agent but the Lead.

use crate::error::ApiError;
use crate::routes::stream::StreamHub;
use axum::http::StatusCode;
use orch_config::agents::{self, AgentKind, Registry, Role};
use orch_config::discovery::skills::{self, AssetState};
use orch_config::launch::launch_command;
use orch_config::models::Catalog;
use orch_config::tunables::Tunables;
use orch_core::paths::ORCH_DIR;
use orch_core::store::{DB_FILE, Store};
use orch_herdr::launch::{AgentStart, Started, name_when_seen, start_agent_in_pane};
use orch_herdr::{AgentInfo, AgentStatus, HerdrError, RuntimeAdapter};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use utoipa::ToSchema;

/// Error code: a Lead already runs or is starting.
pub const LEAD_RUNNING: &str = "lead_running";
/// Error code: the agent kind is unknown or not installed on this machine.
pub const AGENT_NOT_INSTALLED: &str = "agent_not_installed";
/// Error code: Herdr is not installed, or it is not running and could not be started.
pub const HERDR_UNAVAILABLE: &str = "herdr_unavailable";
/// The name a Lead gets before a run names one (the plan's `Lead:` line).
pub const DEFAULT_LEAD_NAME: &str = "lead";
/// Error code: every name a Lead could take is an agent of another project in Herdr.
pub const LEAD_NAME_TAKEN: &str = "lead_name_taken";

/// Where the Lead stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LeadState {
    /// No Lead: none was started, and herdr has none.
    None,
    /// A start was asked for and is under way.
    Starting,
    /// herdr has the Lead.
    Running,
    /// There was a Lead (started here, or named by the run) and herdr no longer has it.
    Gone,
}

/// What a running Lead's pane is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LeadStatus {
    Idle,
    Working,
    Blocked,
}

/// The Lead's state: the answer of `GET /lead` and `POST /lead/start`, and the payload of
/// the stream's `lead` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LeadView {
    pub state: LeadState,
    /// The Lead's agent kind (`claude`, `codex`, ...), when there is or was one.
    pub agent: Option<String>,
    /// Its Herdr pane id, once it has one.
    pub pane: Option<String>,
    /// For a running Lead: what its pane is doing (`null` when herdr cannot tell).
    pub status: Option<LeadStatus>,
    /// What is happening, or what went wrong and what to do about it.
    pub note: Option<String>,
}

/// Body of `POST /lead/start`.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct LeadStartBody {
    /// The agent kind to start as the Lead (`claude`, `codex`, ...): installed, with the
    /// `lead` role.
    #[serde(default)]
    pub agent: String,
    /// The developer's first message, handed over with the playbook exactly as written.
    #[serde(default)]
    pub message: Option<String>,
}

/// How the launcher finds agents and how long it waits.
#[derive(Debug, Clone)]
pub struct LeadOptions {
    /// The agent registry; `None` reads it (with the user's overrides) on every start.
    pub registry: Option<Registry>,
    /// The model catalog; `None` reads it on every start.
    pub catalog: Option<Catalog>,
    /// The home directory; `None` reads `$HOME`.
    pub home: Option<PathBuf>,
    /// Seconds Herdr has to come up after the launcher started it.
    pub server_wait_s: f64,
    /// Seconds herdr has to see the agent start (`ORCH_TEAM_WAIT_S`, default 90).
    pub agent_wait_s: f64,
    /// Seconds the started agent has to become ready for its first prompt.
    pub ready_wait_s: f64,
    /// How often a page with the stream open sees the Lead's pane checked.
    pub watch_every: Duration,
}

impl LeadOptions {
    /// From the tunables: `ORCH_TEAM_WAIT_S` bounds the agent's start and its readiness.
    pub fn from_tunables(t: &Tunables) -> LeadOptions {
        LeadOptions {
            registry: None,
            catalog: None,
            home: None,
            server_wait_s: 15.0,
            agent_wait_s: t.team_wait_s.max(0.0),
            ready_wait_s: t.team_wait_s.max(0.0),
            watch_every: Duration::from_secs(2),
        }
    }
}

impl Default for LeadOptions {
    fn default() -> Self {
        let t = Tunables::from_lookup(|_| None).expect("defaults always parse");
        LeadOptions::from_tunables(&t)
    }
}

/// A start under way.
#[derive(Debug, Clone)]
struct Starting {
    kind: String,
    pane: Option<String>,
    note: String,
}

#[derive(Debug, Default)]
struct Slot {
    /// The start under way, if any. Set and checked under the lock: this is what makes a
    /// second start impossible.
    starting: Option<Starting>,
    /// The kind of the last Lead this server started (so its absence reads `gone`).
    started: Option<String>,
    /// The pane it was started in.
    pane: Option<String>,
    /// What the last start left to say (a failure, or a prompt that could not be sent).
    note: Option<String>,
    /// The last start's command runs in `pane`, but herdr never saw the agent start (often a
    /// first-run question): the next start of the same kind picks that pane up instead of
    /// opening another agent.
    unseen: bool,
    /// The agent name the last start gave the Lead (see `LeadLauncher::claim_name`).
    name: Option<String>,
}

/// The project's Lead launcher (one per served project, shared by every request).
pub struct LeadLauncher {
    root: PathBuf,
    herdr: Arc<dyn RuntimeAdapter + Send + Sync>,
    opts: LeadOptions,
    slot: Mutex<Slot>,
    hub: Mutex<Option<Arc<StreamHub>>>,
    published: Mutex<Option<LeadView>>,
    watching: AtomicBool,
}

fn herdr_unavailable(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::SERVICE_UNAVAILABLE, HERDR_UNAVAILABLE, message)
}

fn pane_status(s: AgentStatus) -> Option<LeadStatus> {
    match s {
        AgentStatus::Working => Some(LeadStatus::Working),
        AgentStatus::Blocked => Some(LeadStatus::Blocked),
        AgentStatus::Idle | AgentStatus::Done => Some(LeadStatus::Idle),
        AgentStatus::Unknown | AgentStatus::Unwatchable | AgentStatus::Gone => None,
    }
}

/// The one prompt a new Lead gets: that kind's planning skill command plus the developer's
/// first message as written (or waiting instructions when there is none yet), and how it was
/// started in Herdr. The full playbook is no longer typed into the pane.
pub fn first_prompt(
    kind: &str,
    name: &str,
    pane: &str,
    message: Option<&str>,
    invoke: Option<&str>,
) -> String {
    let header = format!(
        "\n---\nThe herdr-orchestra app started you in Herdr pane {pane} and already named it \
`{name}`: skip `orch lead whoami`, and write `Lead: {name} ({kind})` in the plan header.\n"
    );
    match message {
        Some(m) => {
            let cmd = match invoke {
                Some(inv) if inv.contains("<feature>") => inv.replace("<feature>", m),
                Some(inv) => format!("{inv} {m}"),
                None => format!("use the orch-plan skill: {m}"),
            };
            format!("{cmd}{header}")
        }
        None => format!(
            "---\nThe herdr-orchestra app started you in Herdr pane {pane} and already named it \
`{name}`: skip `orch lead whoami`, and write `Lead: {name} ({kind})` in the plan header.\n\n\
The developer has not written anything yet: say that you are ready, then wait for them.\n"
        ),
    }
}

/// Whether the kind's orch skills and slash commands are already installed.
fn skills_installed(ak: &AgentKind, home: &Path) -> bool {
    let assets = skills::agent_assets(ak, home);
    if assets.is_empty() {
        return true;
    }
    assets.iter().all(|a| a.state != AssetState::Missing)
}

/// Install the kind's orch skills and slash commands through orch's own skill installer.
fn install_skills(ak: &AgentKind, home: &Path) -> Result<(), String> {
    let flavor = ak
        .commands_from
        .as_deref()
        .unwrap_or(skills::DEFAULT_FLAVOR);
    let mut recs = Vec::new();
    for d in &ak.skills_dirs {
        let path = skills::expand(d, home);
        let (_, r) = skills::sync_skills(&path)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        recs.extend(r);
    }
    for d in &ak.commands_dirs {
        let path = skills::expand(d, home);
        let (_, r) = skills::sync_commands(&path, flavor)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        recs.extend(r);
    }
    if !recs.is_empty() {
        skills::manifest_record(home, &recs)
            .map_err(|e| format!("cannot write {}: {e}", skills::manifest_path(home).display()))?;
    }
    Ok(())
}

impl LeadLauncher {
    pub fn new(
        root: PathBuf,
        herdr: Arc<dyn RuntimeAdapter + Send + Sync>,
        opts: LeadOptions,
    ) -> LeadLauncher {
        LeadLauncher {
            root,
            herdr,
            opts,
            slot: Mutex::new(Slot::default()),
            hub: Mutex::new(None),
            published: Mutex::new(None),
            watching: AtomicBool::new(false),
        }
    }

    /// The run's `Lead:` name, when a run names one.
    fn run_lead_name(&self) -> Option<String> {
        let db = self.root.join(ORCH_DIR).join(DB_FILE);
        db.is_file()
            .then(|| Store::open(&db).ok()?.lead_name().ok().flatten())
            .flatten()
    }

    /// The name this project's Lead takes when `lead` is already another project's: Herdr
    /// agent names are global, and chat reaches the Lead by its bare name, so two Leads must
    /// never share one. Stable for a folder, so a restarted app finds its Lead again.
    fn own_name(&self) -> String {
        let path = self.root.to_string_lossy();
        // FNV-1a: the same folder gives the same name in every run
        let mut h: u64 = 0xcbf29ce484222325;
        for b in path.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
        }
        let base: String = self
            .root
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let base = base.trim_matches('-');
        let base: String = base.chars().take(24).collect();
        if base.is_empty() {
            format!("{DEFAULT_LEAD_NAME}-{:08x}", h & 0xffff_ffff)
        } else {
            format!("{DEFAULT_LEAD_NAME}-{base}-{:08x}", h & 0xffff_ffff)
        }
    }

    /// The names this project's Lead may have: the run's `Lead:` name when a run names one,
    /// else `lead`, else the project's own name.
    fn candidate_names(&self) -> Vec<String> {
        match self.run_lead_name() {
            Some(n) => vec![n],
            None => vec![DEFAULT_LEAD_NAME.to_string(), self.own_name()],
        }
    }

    /// The Lead's agent name: the run's `Lead:` line when a run names one; else the name the
    /// last start gave it; else whichever candidate name herdr has an agent of in this
    /// project's folder; else `lead`.
    pub fn lead_name(&self) -> String {
        if let Some(n) = self.run_lead_name() {
            return n;
        }
        if let Some(n) = self.slot.lock().unwrap().name.clone() {
            return n;
        }
        self.candidate_names()
            .into_iter()
            .find(|n| self.project_agent(n).is_ok())
            .unwrap_or_else(|| DEFAULT_LEAD_NAME.to_string())
    }

    /// Whether the run names a lead (then a missing Lead is `gone`, not `none`).
    fn run_has_lead(&self) -> bool {
        let db = self.root.join(ORCH_DIR).join(DB_FILE);
        db.is_file()
            && Store::open(&db)
                .ok()
                .and_then(|s| s.lead_name().ok().flatten())
                .is_some()
    }

    /// The agent herdr has under `name`, when it runs in this project's folder. Herdr agent
    /// names are global, so a Lead of another served project is not this project's Lead:
    /// it counts as `AgentNotFound`. An agent whose folder herdr does not report is taken
    /// as ours.
    fn project_agent(&self, name: &str) -> Result<AgentInfo, HerdrError> {
        let a = self.herdr.agent(name)?;
        match a.cwd.as_deref() {
            Some(cwd) if !same_folder(Path::new(cwd), &self.root) => {
                Err(HerdrError::AgentNotFound {
                    agent: name.to_string(),
                    message: format!("agent `{name}` runs in {cwd}, not in this project"),
                })
            }
            _ => Ok(a),
        }
    }

    /// Where the Lead stands now. Blocking: it asks herdr.
    pub fn view(&self) -> LeadView {
        let (started, last_pane, last_note) = {
            let slot = self.slot.lock().unwrap();
            if let Some(s) = &slot.starting {
                return LeadView {
                    state: LeadState::Starting,
                    agent: Some(s.kind.clone()),
                    pane: s.pane.clone(),
                    status: None,
                    note: Some(s.note.clone()),
                };
            }
            (slot.started.clone(), slot.pane.clone(), slot.note.clone())
        };
        let name = self.lead_name();
        let missing = |note: String| {
            let had_one = started.is_some() || self.run_has_lead();
            LeadView {
                state: if had_one {
                    LeadState::Gone
                } else {
                    LeadState::None
                },
                agent: started.clone(),
                pane: if had_one { last_pane.clone() } else { None },
                status: None,
                note: Some(note),
            }
        };
        match self.project_agent(&name) {
            Ok(a) if a.status != AgentStatus::Gone => LeadView {
                state: LeadState::Running,
                agent: Some(a.kind),
                pane: Some(a.pane_id.clone()),
                status: pane_status(a.status),
                // what the start left to say belongs to the pane it started
                note: last_note.filter(|_| last_pane.as_deref() == Some(a.pane_id.as_str())),
            },
            Ok(_) | Err(HerdrError::AgentNotFound { .. }) => {
                missing(last_note.unwrap_or_else(|| {
                    if started.is_some() || self.run_has_lead() {
                        format!("Herdr has no agent named `{name}` any more: start the Lead again.")
                    } else {
                        "No Lead yet: it starts with your first message.".to_string()
                    }
                }))
            }
            Err(e) => {
                let reachable = self
                    .herdr
                    .availability()
                    .map(|a| a.reachable)
                    .unwrap_or(false);
                missing(if reachable {
                    format!("Herdr did not answer about the Lead ({e}).")
                } else {
                    "Herdr is not running; starting the Lead starts it.".to_string()
                })
            }
        }
    }

    /// Start the Lead. Blocking: checks the kind and Herdr (starting Herdr when it is not
    /// running, bounded), claims the one Lead slot, then leaves the agent's start to a thread
    /// and answers `starting`. `message` is the developer's first message, already cleaned of
    /// the server's token.
    pub fn start(
        self: &Arc<Self>,
        kind: &str,
        message: Option<String>,
    ) -> Result<LeadView, ApiError> {
        let kind = kind.trim().to_string();
        if kind.is_empty() {
            return Err(ApiError::bad_request(
                "`agent` is required: the agent kind to start as the Lead, e.g. `claude`",
            ));
        }
        let (command, invoke) = self.check_kind(&kind)?;
        {
            let mut slot = self.slot.lock().unwrap();
            if let Some(s) = &slot.starting {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    LEAD_RUNNING,
                    format!("a Lead ({}) is already starting: wait for it", s.kind),
                ));
            }
            slot.starting = Some(Starting {
                kind: kind.clone(),
                pane: None,
                note: "checking Herdr".into(),
            });
        }
        let name = match self.ensure_herdr().and_then(|()| self.claim_name()) {
            Ok(n) => n,
            Err(e) => {
                self.slot.lock().unwrap().starting = None;
                return Err(e);
            }
        };
        self.slot.lock().unwrap().name = Some(name.clone());
        let resume = self.unseen_pane(&kind);
        self.set_starting(
            resume.as_deref(),
            &match &resume {
                Some(p) => format!("waiting for {kind} in Herdr pane {p}"),
                None => "opening a Herdr pane in the project folder".into(),
            },
        );
        let view = self.view();
        self.publish();
        let me = Arc::clone(self);
        let message = message.filter(|m| !m.trim().is_empty());
        std::thread::spawn(move || {
            let run = std::panic::AssertUnwindSafe(|| {
                me.launch(&kind, &name, &command, &invoke, message.as_deref(), resume)
            });
            if std::panic::catch_unwind(run).is_err() {
                // never leave the one Lead slot claimed
                me.finish(
                    None,
                    Some("the Lead's start stopped unexpectedly: start the Lead again".into()),
                    false,
                );
            }
        });
        Ok(view)
    }

    /// The pane of the last start of `kind` whose agent herdr never saw, when it is still
    /// open: starting again waits for that agent instead of starting a second one.
    fn unseen_pane(&self, kind: &str) -> Option<String> {
        let pane = {
            let slot = self.slot.lock().unwrap();
            if !slot.unseen || slot.started.as_deref() != Some(kind) {
                return None;
            }
            slot.pane.clone()?
        };
        self.herdr.pane(&pane).ok().map(|p| p.pane_id)
    }

    /// The home directory: `self.opts.home` or `$HOME`.
    pub fn home(&self) -> PathBuf {
        self.opts.home.clone().unwrap_or_else(skills::home)
    }

    /// The kind's launch command and its invoke command, or why it cannot be the Lead.
    fn check_kind(&self, kind: &str) -> Result<(String, String), ApiError> {
        let reg = match &self.opts.registry {
            Some(r) => r.clone(),
            None => agents::load(None)
                .map_err(|e| ApiError::internal(format!("agents registry: {e}")))?,
        };
        let Some(ak) = reg.get(kind) else {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                AGENT_NOT_INSTALLED,
                format!(
                    "agent kind `{kind}` is not known: choose one of {}",
                    reg.kinds().join(", ")
                ),
            ));
        };
        if !ak.roles.contains(&Role::Lead) {
            let roles: Vec<&str> = ak.roles.iter().map(|r| r.as_str()).collect();
            return Err(ApiError::bad_request(format!(
                "agent kind `{kind}` cannot be the Lead: its roles are {}",
                if roles.is_empty() {
                    "none".into()
                } else {
                    roles.join(", ")
                }
            )));
        }
        if !ak.is_installed() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                AGENT_NOT_INSTALLED,
                format!(
                    "agent kind `{kind}` is not installed on this machine (no `{}` on PATH): \
install it, or choose an installed agent",
                    ak.binary
                ),
            ));
        }
        if !ak.herdr_detect {
            return Err(ApiError::bad_request(format!(
                "agent kind `{kind}` cannot be the Lead: Herdr cannot tell when it is ready or \
working (herdr_detect = false)"
            )));
        }
        let Some(invoke) = ak.invoke.as_deref().filter(|s| !s.trim().is_empty()) else {
            return Err(ApiError::bad_request(format!(
                "agent kind `{kind}` cannot be the Lead: no `invoke` command configured in the registry"
            )));
        };
        let invoke = invoke.to_string();
        let home = self.home();
        if !skills_installed(ak, &home) {
            install_skills(ak, &home).map_err(|e| {
                ApiError::bad_request(format!("agent kind `{kind}`: cannot install skills: {e}"))
            })?;
        }
        let cat = match &self.opts.catalog {
            Some(c) => c.clone(),
            None => orch_config::models::load(None)
                .map_err(|e| ApiError::internal(format!("model catalog: {e}")))?,
        };
        let command = launch_command(&reg, &cat, kind, None, false)
            .map(|l| l.command)
            .map_err(|e| ApiError::bad_request(format!("agent kind `{kind}`: {e}")))?;
        Ok((command, invoke))
    }

    /// Herdr is reachable, or it was started headless and came up within the bound.
    fn ensure_herdr(&self) -> Result<(), ApiError> {
        let up = |h: &dyn RuntimeAdapter| match h.availability() {
            Ok(a) => Ok(a.reachable),
            Err(e) => Err(e),
        };
        match up(self.herdr.as_ref()) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(e) => {
                return Err(herdr_unavailable(format!(
                    "Herdr did not answer ({e}): check that Herdr works (`herdr status`), then start the Lead again"
                )));
            }
        }
        self.set_starting(None, "starting Herdr");
        if let Err(e) = self.herdr.start_server() {
            return Err(herdr_unavailable(match e {
                HerdrError::Unavailable { message } => format!(
                    "Herdr is not installed or cannot be run ({message}): install Herdr \
(https://herdr.dev), then start the Lead again"
                ),
                e => format!(
                    "Herdr is not running and could not be started ({e}): start Herdr yourself \
(`herdr`), then start the Lead again"
                ),
            }));
        }
        let deadline = Instant::now() + Duration::from_secs_f64(self.opts.server_wait_s);
        loop {
            if let Ok(true) = up(self.herdr.as_ref()) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(herdr_unavailable(format!(
                    "Herdr is not running, and it did not come up within {}s after the app started \
it (`herdr server`): start Herdr yourself (`herdr`), then start the Lead again",
                    self.opts.server_wait_s
                )));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Make sure no Lead runs in this project, then pick the name the new one takes: the
    /// first candidate name no agent in Herdr has (in any folder, since names are global).
    fn claim_name(&self) -> Result<String, ApiError> {
        let unavailable = |e: HerdrError| {
            herdr_unavailable(format!(
                "Herdr did not answer whether a Lead already runs ({e}): check that Herdr works \
(`herdr status`), then start the Lead again"
            ))
        };
        let names = self.candidate_names();
        for name in &names {
            match self.project_agent(name) {
                Ok(a) if a.status != AgentStatus::Gone => {
                    return Err(ApiError::new(
                        StatusCode::CONFLICT,
                        LEAD_RUNNING,
                        format!(
                            "a Lead is already running: `{name}` ({}) in Herdr pane {}",
                            a.kind, a.pane_id
                        ),
                    ));
                }
                Ok(_) | Err(HerdrError::AgentNotFound { .. }) => {}
                Err(e) => return Err(unavailable(e)),
            }
        }
        for name in &names {
            match self.herdr.agent(name) {
                Ok(a) if a.status != AgentStatus::Gone => {} // another project's agent
                Ok(_) | Err(HerdrError::AgentNotFound { .. }) => return Ok(name.clone()),
                Err(e) => return Err(unavailable(e)),
            }
        }
        Err(ApiError::new(
            StatusCode::CONFLICT,
            LEAD_NAME_TAKEN,
            format!(
                "Herdr already has an agent named `{}` in another folder, and chat reaches the \
Lead by that name: close that agent (or its project's Lead) in Herdr, then start the Lead again",
                names
                    .last()
                    .map(String::as_str)
                    .unwrap_or(DEFAULT_LEAD_NAME)
            ),
        ))
    }

    fn set_starting(&self, pane: Option<&str>, note: &str) {
        if let Some(s) = self.slot.lock().unwrap().starting.as_mut() {
            if pane.is_some() {
                s.pane = pane.map(str::to_string);
            }
            s.note = note.to_string();
        }
    }

    /// End the start: record what to say and which Lead (if any) now exists, then tell the
    /// page.
    fn finish(&self, started: Option<(&str, &str)>, note: Option<String>, unseen: bool) {
        {
            let mut slot = self.slot.lock().unwrap();
            slot.starting = None;
            slot.note = note;
            slot.unseen = unseen;
            if let Some((kind, pane)) = started {
                slot.started = Some(kind.to_string());
                slot.pane = Some(pane.to_string());
            }
        }
        self.publish();
    }

    /// The slow part of a start, on its own thread; every wait is bounded. `resume` is the
    /// pane of an earlier start whose agent herdr never saw: nothing new is opened or typed
    /// there, the agent is only waited for again.
    fn launch(
        &self,
        kind: &str,
        name: &str,
        command: &str,
        invoke: &str,
        message: Option<&str>,
        resume: Option<String>,
    ) {
        let (pane, started) = match resume {
            Some(pane) => {
                let seen = name_when_seen(
                    self.herdr.as_ref(),
                    &pane,
                    name,
                    self.opts.agent_wait_s,
                    &mut std::thread::sleep,
                );
                (
                    pane,
                    if seen {
                        Started::Named
                    } else {
                        Started::NotSeen
                    },
                )
            }
            None => {
                let label = self
                    .root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned());
                let pane = match self.herdr.create_workspace(&self.root, label.as_deref()) {
                    Ok(p) => p.pane_id,
                    Err(e) => {
                        return self.finish(
                            None,
                            Some(format!(
                                "Herdr could not open a pane for the Lead ({e}): check that Herdr works, then start the Lead again"
                            )),
                            false,
                        );
                    }
                };
                self.set_starting(
                    Some(&pane),
                    &format!("starting {kind} in Herdr pane {pane}"),
                );
                self.publish();
                let started = start_agent_in_pane(
                    self.herdr.as_ref(),
                    &AgentStart {
                        pane: &pane,
                        command,
                        name,
                        detect: true,
                        wait_s: self.opts.agent_wait_s,
                    },
                    &mut std::thread::sleep,
                );
                (pane, started)
            }
        };
        match started {
            Started::Named | Started::Unwatched => {}
            Started::RunFailed(e) => {
                return self.finish(
                    None,
                    Some(format!(
                        "Herdr opened pane {pane}, but `{command}` could not be run in it ({e}): start the Lead again"
                    )),
                    false,
                );
            }
            Started::NotSeen => {
                return self.finish(
                    Some((kind, &pane)),
                    Some(format!(
                        "Herdr pane {pane} runs `{command}`, but Herdr did not see {kind} start within {}s: \
it may be asking a first-run question (trust this folder, sign in). Answer it in that pane, \
then start the Lead again: it picks that pane up",
                        self.opts.agent_wait_s
                    )),
                    true,
                );
            }
        }
        // the pane carries the Lead's name too, so the developer finds it in Herdr
        let _ = self.herdr.rename_pane(&pane, name);
        self.set_starting(None, &format!("waiting for {kind} to be ready"));
        self.publish();
        let prompt = first_prompt(kind, name, &pane, message, Some(invoke));
        let note = self.hand_over(name, &prompt);
        self.finish(Some((kind, &pane)), note, false);
    }

    /// Type the first prompt once the Lead is ready (idle), within the bound. `None` when it
    /// was sent; else what went wrong.
    fn hand_over(&self, name: &str, prompt: &str) -> Option<String> {
        let deadline = Instant::now() + Duration::from_secs_f64(self.opts.ready_wait_s);
        let mut last: String;
        loop {
            match self.project_agent(name) {
                Ok(a) if matches!(a.status, AgentStatus::Idle | AgentStatus::Done) => {
                    match self.herdr.send_prompt(name, prompt) {
                        Ok(()) => return None,
                        Err(e @ HerdrError::AgentBlocked { .. }) => last = e.to_string(),
                        Err(e) => {
                            return Some(format!(
                                "the Lead started, but the playbook and your first message could not \
be typed into it ({e}): send your message again"
                            ));
                        }
                    }
                }
                Ok(a) => last = a.status.to_string(),
                Err(HerdrError::AgentNotFound { .. }) => {
                    return Some(
                        "the Lead exited before it got the playbook: start the Lead again".into(),
                    );
                }
                Err(e) => last = e.to_string(),
            }
            if Instant::now() >= deadline {
                return Some(format!(
                    "the Lead did not become ready within {}s ({last}), so the playbook and your \
first message were not sent: it may be asking a question in its pane; answer it, then send your \
message again",
                    self.opts.ready_wait_s
                ));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Publish the Lead's state on the live stream as a `lead` event when it changed since
    /// the last one. Blocking: it asks herdr.
    pub fn publish(&self) {
        let Some(hub) = self.hub.lock().unwrap().clone() else {
            return;
        };
        // one publisher at a time, so an older view never overtakes a newer one
        let mut last = self.published.lock().unwrap();
        let view = self.view();
        if last.as_ref() != Some(&view) {
            hub.publish("lead", serde_json::to_value(&view).unwrap_or_default());
            *last = Some(view);
        }
    }

    /// Send state changes to `hub`'s stream from now on, and watch the Lead's pane (every
    /// `watch_every`) until `shutdown`, so the page learns of changes without polling.
    /// Called once the page asks about the Lead; later calls change nothing.
    pub fn watch(
        self: &Arc<Self>,
        hub: Arc<StreamHub>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        *self.hub.lock().unwrap() = Some(hub);
        if self.watching.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let m = Arc::clone(&me);
                if tokio::task::spawn_blocking(move || m.publish())
                    .await
                    .is_err()
                {
                    break;
                }
                tokio::select! {
                    _ = tokio::time::sleep(me.opts.watch_every) => {}
                    // the sender dropping (the server is gone) ends the watch too, else
                    // `changed()` would fail at once on every pass and spin on herdr
                    changed = shutdown.changed() => {
                        if changed.is_err() {
                            break;
                        }
                    }
                }
                if *shutdown.borrow() {
                    break;
                }
            }
            me.watching.store(false, Ordering::SeqCst);
        });
    }
}

/// Whether two paths name the same folder (symlinks and a trailing `/` do not matter).
fn same_folder(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| {
        p.canonicalize()
            .unwrap_or_else(|_| p.components().collect())
    };
    canon(a) == canon(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orch_herdr::FakeHerdr;
    use orch_herdr::PaneInfo;

    fn launcher(root: &Path, fake: Arc<FakeHerdr>) -> Arc<LeadLauncher> {
        let opts = LeadOptions {
            watch_every: Duration::from_millis(20),
            ..LeadOptions::default()
        };
        Arc::new(LeadLauncher::new(root.to_path_buf(), fake, opts))
    }

    // The watcher ends when the server's shutdown sender is dropped, instead of spinning on
    // herdr calls without a pause.
    #[tokio::test]
    async fn the_watch_ends_when_the_shutdown_sender_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let l = launcher(dir.path(), Arc::new(FakeHerdr::new()));
        let (tx, rx) = tokio::sync::watch::channel(false);
        l.watch(Arc::new(StreamHub::new(dir.path().to_path_buf())), rx);
        assert!(l.watching.load(Ordering::SeqCst));
        drop(tx);
        let deadline = Instant::now() + Duration::from_secs(5);
        while l.watching.load(Ordering::SeqCst) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            !l.watching.load(Ordering::SeqCst),
            "the watch task kept running"
        );
    }

    // Herdr agent names are global: a Lead running in another project's folder is not this
    // project's Lead.
    #[test]
    fn a_lead_in_another_folder_is_not_this_projects_lead() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("mine");
        let other = dir.path().join("other");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let fake = Arc::new(FakeHerdr::new());
        let mut pane = PaneInfo::new("w1:p1", "w1", "w1:t1");
        pane.cwd = Some(other.display().to_string());
        fake.add_pane(pane);
        fake.add_agent(AgentInfo::new("lead", "claude", "w1:p1", AgentStatus::Idle));
        let l = launcher(&mine, fake.clone());
        assert_eq!(l.view().state, LeadState::None);
        assert!(l.claim_name().is_ok());

        // the same Lead, seen from its own folder (even through a trailing slash), runs
        let l = launcher(&other, fake);
        assert_eq!(l.view().state, LeadState::Running);
        assert!(l.claim_name().is_err());
        let slashed = PathBuf::from(format!("{}/", other.display()));
        assert!(same_folder(&slashed, &other));
    }
}
