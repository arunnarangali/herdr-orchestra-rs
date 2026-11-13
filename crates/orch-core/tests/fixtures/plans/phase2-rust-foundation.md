# Plan: Phase 2 — Rust Foundation
Branch: feature/rust-foundation
Check command: test ! -f Cargo.toml || (cargo build --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace)
Setup command: test ! -f Cargo.toml || cargo fetch
Isolation: worktree
Max parallel: 2
Scope: warn
Lead: lead (claude: opus)
Workers: claude-1 (claude: sonnet), opencode-1 (opencode: opencode/glm-5.3-flash-free)
Reviewer: reviewer (agy: gemini-3.8-flash)

## Global instructions
- The product spec is `docs/plan.md` in this repo. Phase 2 is §24 "Phase 2 — Rust Foundation"; the task states are §11, storage §16, CLI §17, agents §5, models §5A, stack §21, engineering rules §25.
- The behaviour reference is the installed Python edition v4.10.1 at `~/.local/share/herdr-orchestra/v4.10.1/` (`bin/orch`, `agents.default.toml`, `models.default.toml`). Do NOT use `/mnt/workspace/projects/herdr-orchestra`: that checkout is the old v4.4.0.
- Must not: any Herdr integration (no calls to the `herdr` binary, no pane or agent handling), any GUI or REST API, or anything that belongs to Phases 3–11 (plan parsing / `orch sync` validation, scheduler, event manager, git and worktrees, verification, review, recovery, installer, release).
- Done when `cargo build`, `cargo test` and `cargo clippy` (warnings denied) pass for the whole workspace.
- Stable Rust, edition 2024. Unit tests are mandatory for everything that has behaviour. Tests never need a real agent, network access or a real Herdr.
- Only M1 adds or changes dependencies (root and crate `Cargo.toml`, `Cargo.lock`). If a later milestone truly needs another crate, ask with a QUESTION instead of adding it.
- Keep it simple (§25 rule 21): no async runtime, no extra crates beyond the layout below.

## Feature breakdown
- Workspace and CLI skeleton: root Cargo workspace, the crate layout, every dependency, the shared error type, logging setup, locating the project's `.orchestrate/` folder, and an `orch` binary that knows every command name from today's Python `orch` (all stubs except `version` and help).
- Task model and storage: typed task states, the §11 transition table and the one transition function, the record each transition leaves, and SQLite storage (migrations, tasks, transition history, events written in the same transaction as the state change).
- Configuration and driver lease: loading the agent registry and model catalog (shipped defaults merged key by key with the user's override files, deny policy, tier floors), the `ORCH_*` tunables, and the single-driver lease on `.orchestrate/driver.lock`.
- How they connect: the skeleton is the foundation (it owns every hotspot file: workspace manifest, lockfile, crate manifests, `lib.rs` module lists). Task/storage and config/lease touch different crates and modules and don't need each other, so they run in parallel on the two coders once the skeleton has landed. Three milestones.

## [x] M1: Workspace, crate layout, CLI skeleton, errors and logging
Depends on: none
Why separate: foundation — it owns every shared hotspot file (workspace and crate manifests, `Cargo.lock`, `lib.rs` module lists, the error type), so M2 and M3 can run in parallel without touching the same files
Prefer: claude-1
Tier: medium
Needs: coding, tool-use
Model why: the crate layout and command list are fixed below, so this is careful but well-specified setup work, not design
Review: yes
Likely files: Cargo.toml, Cargo.lock, rust-toolchain.toml, crates/orch/, crates/orch-core/, crates/orch-config/, .gitignore
Goal: A Cargo workspace that builds, lints clean and tests green, with the agreed crate layout, every Phase 2 dependency in place, a shared error type, logging, and an `orch` binary that accepts every command name today's Python `orch` accepts.
Why: Every later phase builds on this layout. Fixing it first lets the task/storage and config/lease work run in parallel without editing the same files.
Acceptance criteria:
- `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass from a clean checkout.
- The workspace has exactly the three crates listed under Interfaces, with the module files listed there present (empty or placeholder is fine for modules M2/M3 own) and declared in each crate's `lib.rs`.
- `orch` with no arguments prints full usage listing every command and exits 0; an unknown command prints usage and exits 2 (same as Python).
- `orch version` (also `--version`, `-v`) prints the version from the workspace manifest (5.0.0-dev or similar pre-release of 5.0.0) and exits 0.
- Every command name in the Python v4.10.1 `COMMANDS` map, plus `lead plan|run|check|whoami`, `agents` (with `launch`, `sync`, `check`), `doctor`, `models` (with `show`, `check`), `team` (with `up`), is accepted by the parser and has a help line; each not-yet-built command prints a clear "not implemented in this build" message on stderr and exits with one documented exit code that is not 0, 2, 3, 4, 5, 6, 7 or 99 (those already mean something in Python).
- A unit or CLI test asserts the full list of command names, so a missing name fails the build.
- Logging goes to stderr through `tracing`; its level is controlled by one documented environment variable and is quiet by default; secrets are never logged.
- A function in `orch-core` finds the main repository root (also when called from inside a linked git worktree, as Python's `main_root()` does with `git rev-parse --git-common-dir`) and the `.orchestrate/` folder under it, with a clear error outside a git repo; it has tests.
Constraints:
- Use exactly the crate names and module ownership under Interfaces.
- Dependencies (declared once as workspace dependencies): clap (derive), serde, serde_json, toml, thiserror, anyhow, tracing, tracing-subscriber, rusqlite with its bundled SQLite (sync, single static binary; no Tokio or SQLx in Phase 2), fs2 or fd-lock for the file lease, globset; tempfile as a dev-dependency. Nothing else unless it is clearly needed for the acceptance criteria above.
- Commit `Cargo.lock` (this is a binary workspace).
Interfaces:
- `crates/orch` — binary `orch`: CLI parsing and command dispatch only; no business logic.
- `crates/orch-core` — library. Modules: `error` (M1: the shared error type, thiserror-based, and a Result alias), `paths` (M1: main repo root and `.orchestrate/` location), `logging` (M1), `task` (M2), `store` (M2), `lease` (M3).
- `crates/orch-config` — library, depends on `orch-core` only for its error type. Modules: `agents` (M3), `models` (M3), `tunables` (M3).
- The error type in `orch-core::error` is the one error the CLI turns into a message and an exit code; M2 and M3 add variants to it only inside their own modules' error enums that convert into it (so they don't both edit `error.rs`). Say in `error.rs` how a module plugs its own error in.
Context:
- The repo has no Rust code yet: only `docs/plan.md` (spec), `redme.md`, `the_main_plan.md` (older plan), `LICENSE`, `.gitignore` (already ignores `target/`, `.orchestrate/`, `.orch-task/`).
- Python command map: `COMMANDS` at about line 3589 of `~/.local/share/herdr-orchestra/v4.10.1/bin/orch`; usage text `USAGE` right after it; no-repo commands (`lead`, `agents`, `doctor`, `models`, `team`, `version`) at about line 3973. Spec §17 lists the same commands by group.
- Python exit codes in use: 1 general error, 2 usage / validation error, 3 needs a human, 4 and 7 merge outcomes, 5 delivery failed, 6 integration-fix, 99 test fault hook.
Out of scope:
- Any real behaviour behind commands other than `version` and help. New Rust-only commands from §17 ("New commands to add") are Phase 4+.
- Task states, storage, config loading and the lease (M2, M3).
Open questions:
- none
Notes from user:

## [x] M2: Typed task model, §11 transition table, SQLite storage
Depends on: M1 (the workspace, `orch-core` crate with its empty `task` and `store` modules, the error type, and the rusqlite dependency)
Why separate: it can be built in parallel with M3 once M1 lands; it only touches `orch-core::task` and `orch-core::store`
Prefer: claude-1
Tier: high
Needs: reasoning, architecture, coding
Model why: the state model and the database schema are what Phases 3–10 build on (scheduler, event manager, recovery), and a wrong schema or transition rule is expensive to undo
Review: yes
Likely files: crates/orch-core/src/task.rs, crates/orch-core/src/task/, crates/orch-core/src/store.rs, crates/orch-core/src/store/, crates/orch-core/migrations/, crates/orch-core/tests/
Goal: A strongly typed task state machine that matches today's Python transition table exactly, and SQLite storage where every state change is validated and saved together with its history and event in one transaction.
Why: The scheduler, event manager and recovery in later phases rely on states never changing outside the allowed table and on no transition being lost after a crash (§11, §16, §25 rules 9 and 12).
Acceptance criteria:
- The 15 states (pending, coding, verifying, awaiting-review, reviewing, reviewed, committed, merged, needs-answer, needs-fix, held, paused, failed, conflict, skipped) are one typed enum whose text forms are exactly the Python strings (kebab-case), round-tripping through serde and the database.
- The allowed transitions equal the §11 table (= Python `TRANSITIONS` in v4.10.1); a unit test checks every one of the 15×15 pairs against the table, so any drift fails.
- `merged` is terminal; a move out of it is refused with a message saying merged is final. A move to the same state is a no-op that records nothing unless a reason is given (Python `move()` behaviour).
- Manual changes (what `orch set` will use) are allowed only to pending, paused, failed and skipped; any other target needs an explicit force flag, and a forced transition is recorded as forced.
- Each transition record keeps: previous state, new state, timestamp, triggering event / reason, worker and model, attempt, and whether it was forced. Each task keeps its round counters: fix, review, conflict, lost attempts, escalation used.
- `Ready` (pending with all dependencies merged) and `Assigned` (coding with a worker) are available as derived checks, not stored states.
- SQLite storage under `.orchestrate/state.db` with versioned migrations applied on open; opening an existing database at the current version is a no-op; a database from a newer schema version is refused with a clear error (like Python's "written by a newer orch").
- Storing a transition writes the new state, the history row and the event row in one transaction: a test that makes the event write fail shows the task state unchanged.
- Two connections changing the same task concurrently never lose an update (a test with two connections / threads shows both transitions validated and recorded, or the second refused, never silently overwritten).
- Tests use temporary directories; none touch the real repo's `.orchestrate/`.
Constraints:
- Only `orch-core` modules `task` and `store` (plus their tests and migrations). Don't edit `orch-core/src/error.rs` or `lib.rs`; plug your errors in the way `error.rs` describes. Don't add dependencies.
- No other code path may change a task's state: storage exposes transitions, not "set state".
- Schema covers what Phase 2 needs: tasks (with dependencies and counters), transition history, events. Leave the tables for later phases (workers, sessions, prompt deliveries, merge journal, lead event queue…) to those phases, but the migration scheme must make adding them straightforward.
Interfaces:
- State text forms are the Python strings listed above; `events.jsonl` compatibility (Python writes `{"ts","task","from","to","why","agent","attempt","forced"?}` per line) is a later phase's job, but keep those fields available in the event record.
Context:
- Python reference: `TRANSITIONS`, `SET_OK` and `move()` at about lines 246–285 of `~/.local/share/herdr-orchestra/v4.10.1/bin/orch`; state migration `migrate()` at about line 105 (`STATE_SCHEMA = 1`); task fields include `history` (last 50), `lost`, `attempts`, `worker`/`last_worker`, `attempt`, `reason`.
- `LEAD_BLOCKING = {paused, failed, held, needs-answer}` in Python: those transitions (and to merged) create lead events. The lead queue itself is Phase 6; keep enough in the event record to derive it later.
- Spec: §11 (states, table, what to store), §16 (storage, transactions, recovery).
Out of scope:
- Plan parsing / `orch sync`, scheduling, dispatch, the lead event queue, any CLI wiring of `set`/`status` (stubs stay stubs).
- `state.json` import (decided: a later phase).
Open questions:
- none
Notes from user:
- Decided (option a): no `state.json` import in Phase 2; it comes in a later phase once the plan parser and full schema exist.


## [x] M3: Agent registry and model catalog loading, tunables, driver lease
Depends on: M1 (the workspace, the `orch-config` crate and `orch-core::lease` module, the error type, and the toml / globset / file-lock dependencies)
Why separate: it can be built in parallel with M2 once M1 lands; it only touches `orch-config` and `orch-core::lease`
Prefer: opencode-1
Tier: medium
Needs: coding
Model why: the TOML formats, merge rules and lease behaviour already exist in Python and are copied, not designed
Review: yes
Likely files: crates/orch-config/, crates/orch-core/src/lease.rs, crates/orch-core/src/lease/, crates/orch-core/tests/lease*
Goal: Load the agent registry and the model catalog exactly as the Python edition reads them today (shipped defaults merged key by key with the user's override files), expose the `ORCH_*` tunables with today's defaults, and provide the single-driver lease.
Why: Decision D2 (drop-in replacement) and D3 (shared `agents.default.toml` / `models.default.toml` stay compatible); the lease is the safety rule that exactly one process drives a run (§3, §25 rule 4).
Acceptance criteria:
- The shipped `agents.default.toml` and `models.default.toml` from v4.10.1 are copied into the repo unchanged and embedded in the binary; a test parses both without errors and finds the six agent kinds (claude, codex, amp, opencode, hermes, agy) and every catalog model.
- User overrides are read from `~/.config/herdr-orchestra/agents.toml` / `models.toml`, or from `ORCH_AGENTS_FILE` / `ORCH_MODELS_FILE` when set, and merged key by key per kind / per model (and per key for `[policy]` and `[tiers]`), as Python does; a missing override file is fine; a broken one gives an error naming the file and the problem. Tests cover: override one key of one kind, add a new kind, add a new model, change the deny list.
- Each agent kind exposes today's keys (`binary`, `launch`, `model_flag`, `herdr_detect`, `roles`, `skills_dirs`, `commands_dirs`, `commands_from`, `invoke`, `verified`, `notes`); unknown roles or wrong types are a validation error.
- Each model exposes its kinds, tier, the seven ratings, `context_k`, `open_weights` and `evidence`. Deny patterns are matched case-insensitively as shell-style globs against the model id (`*fable*`, `best`, `*astra*` by default); a test shows `claude-fable-5-2` denied and `sonnet` allowed. A model whose reasoning rating is below its tier's floor (small 1, medium 3, high 4) is reported as invalid.
- Every tunable in Appendix A ("Tunables (environment)") is available with today's default, read from its `ORCH_*` variable; a non-numeric value gives a clear error naming the variable. The fixed limits (3 fix rounds, 2 review rounds, 2 conflict rounds) are exposed as constants.
- Driver lease on `<main repo>/.orchestrate/driver.lock`: taking it while another process holds it fails with a message that names the holder (pid, host, start time); the lock is released by the OS when the holder dies (no stale-lock cleanup needed); another process can ask whether a driver is alive without taking the lease; the holder writes `{"pid","host","started","token"}` JSON as Python does. Tests cover: second taker refused, lease free again after the holder drops it, liveness probe in both states.
Constraints:
- Only `orch-config` and `orch-core::lease` (plus their tests and the two copied TOML files). Don't edit `orch-core/src/error.rs` or `lib.rs`; plug your errors in the way `error.rs` describes. Don't add dependencies.
- Tests set the override paths and `ORCH_*` variables explicitly (or pass them in), so they never read the developer's real `~/.config/herdr-orchestra/` and don't race each other on environment variables.
Interfaces:
- none (M2 does not use config or the lease in Phase 2).
Context:
- Python reference in `~/.local/share/herdr-orchestra/v4.10.1/bin/orch`: tunables at the top of the file (lines ~19–35 and more further down, e.g. `ORCH_LEAD_*`, `ORCH_RISK_MAX_LINES`, `ORCH_MAX_TASKS`); registry loading and merge around lines 437–475; catalog loading around 475–550; lease functions `driver_info`, `driver_alive`, `require_driver`, `take_lease` around lines 155–205 (`fcntl.flock`, exclusive non-blocking to take, shared non-blocking to probe).
- Source files to copy: `~/.local/share/herdr-orchestra/v4.10.1/agents.default.toml` and `models.default.toml` (their header comments document every key).
- Spec: §5 (registry keys), §5A (catalog, tiers, policy), Appendix A (tunables), §3 (single driver).
Out of scope:
- Agent discovery, `doctor`, launching, `team`, skill sync (Phase 4); CLI wiring of `orch models` / `orch agents` (stubs stay stubs); the state lock.
Open questions:
- none
Notes from user:

## [x] M4: Fix for M1: Outside a git repository orch must exit 2, as Python v4.10.1...
Depends on: M1 (a fix to what M1 built)
Why separate: a change the developer asked for after M1 landed.
Review: yes
Likely files: Cargo.toml, Cargo.lock, rust-toolchain.toml, crates/orch/, crates/orch-core/, crates/orch-config/, .gitignore
Tier: medium
Model why: a fix to M1, for the coder who built it.
Goal: Outside a git repository orch must exit 2, as Python v4.10.1 does (its die() defaults to code 2 and main_root() uses it); today Error::NotInGitRepo exits 1 (crates/orch-core/src/error.rs). Found in the M1 review.
Why: the developer asked for this after M1 landed.
Acceptance criteria:
- Outside a git repository orch must exit 2, as Python v4.10.1 does (its die() defaults to code 2 and main_root() uses it); today Error::NotInGitRepo exits 1 (crates/orch-core/src/error.rs). Found in the M1 review.
- nothing else that M1 built changes, and the check command passes
Open questions:
- none

## [x] M5: Fix for M2: State::allowed_targets() must match Python's TRANSITIONS exa...
Depends on: M2 (a fix to what M2 built)
Why separate: a change the developer asked for after M2 landed.
Review: yes
Likely files: crates/orch-core/src/task.rs, crates/orch-core/src/task/, crates/orch-core/src/store.rs, crates/orch-core/src/store/, crates/orch-core/migrations/, crates/orch-core/tests/
Tier: high
Model why: a fix to M2, for the coder who built it.
Goal: State::allowed_targets() must match Python's TRANSITIONS exactly: in Python paused and failed include themselves (via _PARK), as pending already does in Rust; today Paused and Failed omit themselves (crates/orch-core/src/task.rs:106-107). Found in the M2 review.
Why: the developer asked for this after M2 landed.
Acceptance criteria:
- State::allowed_targets() must match Python's TRANSITIONS exactly: in Python paused and failed include themselves (via _PARK), as pending already does in Rust; today Paused and Failed omit themselves (crates/orch-core/src/task.rs:106-107). Found in the M2 review.
- nothing else that M2 built changes, and the check command passes
Open questions:
- none

## [x] M6: Fix for M3: Two fixes from the M3 review: (1) a wrong value type in the ...
Depends on: M3 (a fix to what M3 built)
Why separate: a change the developer asked for after M3 landed.
Review: yes
Likely files: crates/orch-config/, crates/orch-core/src/lease.rs, crates/orch-core/src/lease/, crates/orch-core/tests/lease*
Tier: medium
Model why: a fix to M3, for the coder who built it.
Goal: Two fixes from the M3 review: (1) a wrong value type in the user's models.toml [policy] or [tiers] (e.g. deny = 5) is reported as an error in models.default.toml; the error must name the user's override file (crates/orch-config/src/models.rs, From<toml::de::Error>). (2) The driver lease holder's start time is shown in UTC; it must be local time, as Python's time.localtime() (crates/orch-core/src/lease.rs hhmmss).
Why: the developer asked for this after M3 landed.
Acceptance criteria:
- Two fixes from the M3 review: (1) a wrong value type in the user's models.toml [policy] or [tiers] (e.g. deny = 5) is reported as an error in models.default.toml; the error must name the user's override file (crates/orch-config/src/models.rs, From<toml::de::Error>). (2) The driver lease holder's start time is shown in UTC; it must be local time, as Python's time.localtime() (crates/orch-core/src/lease.rs hhmmss).
- nothing else that M3 built changes, and the check command passes
Open questions:
- none
