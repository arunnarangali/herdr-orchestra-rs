# Plan: Phase 3 — Herdr Integration
Branch: feature/herdr-integration
Check command: cargo build --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
Setup command: cargo fetch
Isolation: worktree
Max parallel: 2
Scope: warn
Lead: lead (claude: opus)
Workers: claude-1 (claude: sonnet), opencode-1 (opencode: opencode/glm-5.3-flash-free)
Reviewer: reviewer (agy: gemini-3.8-flash)

## Global instructions
- The product spec is `docs/plan.md` in this repo. This run is §24 "Phase 3 — Herdr Integration"; the adapter requirements are §4 ("Herdr Runtime Integration"). The relevant recovery rules are in §15 (prompt cannot be delivered; crash mid-step), and the storage rules are in §16 (pending prompt deliveries).
- The behaviour reference is the installed Python edition v4.10.1, `~/.local/share/herdr-orchestra/v4.10.1/bin/orch`:
  - `herdr_state`, `_herdr_run`, `herdr_prompt` and `deliver` (about lines 330–425);
  - the recording of a prompt before it is sent (about line 1612);
  - the recovery of an unconfirmed prompt (about line 1878);
  - `cmd_resend` (about line 1795).
  Do NOT use `/mnt/workspace/projects/herdr-orchestra`: that checkout is the old v4.4.0. Its `tests/sim/herdr` is still a useful picture of what a fake herdr does.
- The real Herdr on this machine is 0.9.5. Its documented interfaces are:
  - `herdr --help`, plus `--help` for each subcommand;
  - `herdr --skill`;
  - `herdr api schema --json` (socket protocol 22; the socket path is in `HERDR_SOCKET_PATH`).
  Never assume undocumented command names, flags or JSON fields. Check them against these sources (§4).
- Must not:
  - any GUI or REST API;
  - any git or worktree handling;
  - the scheduler or event manager (no dispatching, no polling loop, no waking the lead);
  - anything else from Phases 4–11.
  Phase 2 commands that are still stubs stay stubs, except where a milestone says otherwise.
- Done when `cargo test` passes for the whole workspace against the fake herdr, and `cargo build` and `cargo clippy` (warnings denied) pass too.
- Tests never need a real Herdr, a real agent or network access. Running the test suite inside or outside a herdr pane must give the same result.
- Stable Rust, edition 2024. No async runtime (§25 rule 21: keep it simple).
- Only M1 adds or changes dependencies and manifests (root and crate `Cargo.toml`, `Cargo.lock`, workspace members). If a later milestone truly needs another crate, ask with a QUESTION instead of adding it.

## Feature breakdown
- Contract: a new crate for the runtime adapter. It holds:
  - the trait every execution backend implements, covering the §4 capabilities;
  - the typed results (agents, panes, workspaces, agent status, including "not watchable");
  - the error kinds a caller must tell apart (agent not found, stale pane, agent blocked, timed out, herdr missing or failed);
  - an in-memory fake backend that tests can script.
- Herdr CLI backend: implements the trait by running the `herdr` binary, one subprocess per call, with the `ORCH_HERDR_TIMEOUT_S` timeout. It covers every command in the §4 table, including the `@pane:<id>` path for agents herdr cannot detect. It comes with a fake `herdr` executable that mimics 0.9.5's output, so this backend is tested through real subprocess calls. `orch lead check` is wired to it.
- Crash-safe prompt delivery:
  - Each prompt is recorded in SQLite (a new migration) before it is sent, then marked delivered once herdr confirms it.
  - After a crash, an unconfirmed prompt is confirmed from the agent's status or re-sent to the same agent, never to another one.
  - Delivery failures are counted per agent.
  - All of it is built against the trait, so its tests use the in-memory fake.
- How they connect: the trait is the contract both other pieces code against. The CLI backend and delivery touch different files and don't need each other, so they run in parallel on the two coders once the contract has landed. That makes three milestones.

## [x] M1: Runtime adapter contract, typed models and in-memory fake
Depends on: none
Why separate: the CLI backend (M2) and crash-safe delivery (M3) both code against this trait and these types, and both would otherwise edit the same new manifests. Fixing them first lets M2 and M3 run in parallel.
Prefer: claude-1
Tier: high
Needs: architecture, reasoning
Model why: this is the interface every later phase and any future backend builds on. A wrong shape here (sync vs. streaming events, error kinds, how undetectable agents are represented) is expensive to undo.
Review: yes
Likely files: Cargo.toml, Cargo.lock, crates/orch-herdr/
Goal: a new workspace crate that defines the replaceable runtime adapter. It contains the trait covering the §4 capabilities, the typed values it returns, a precise error type, and an in-memory fake backend that tests can script. There is no real Herdr backend yet.
Why: §4 requires the Herdr adapter to be replaceable and testable without Herdr. Every later phase (event manager, team up, recovery) talks to agents only through this contract.
Acceptance criteria:
- The workspace has a new crate for the runtime adapter. `cargo build --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
- The trait covers each §4 adapter capability that maps to the herdr CLI calls listed in §4:
  - runtime availability: am I inside a herdr pane, and is herdr reachable;
  - current pane;
  - list and get agents;
  - agent status;
  - send a prompt;
  - read recent output;
  - rename an agent and rename a pane;
  - get and list panes;
  - list workspaces;
  - split a pane in a given folder;
  - run a command in a pane;
  - send text and keys to a pane.
  Each is documented with the herdr command it corresponds to.
- Agent status is a typed value with at least idle, working, blocked, done, unknown and gone (not found). "Not watchable" (`herdr_detect = false`) is representable, and such an agent is never reported as gone, blocked or idle (§4).
- The error type lets callers distinguish:
  - agent not found;
  - stale or unknown pane id;
  - agent blocked (prompt refused);
  - timed out;
  - herdr not installed or not reachable;
  - any other herdr failure (keeping herdr's message).
  Unit tests show each one can be matched on.
- An in-memory fake implements the trait. Tests can:
  - add and remove agents and panes;
  - set an agent's status;
  - make the next N calls of a given kind fail with a chosen error;
  - read back every prompt and text it was sent, in order.
  It has its own unit tests.
- The crate's module list and manifest already make room for M2's CLI backend and fake `herdr` executable, and for M3's delivery module (see Interfaces), so that M2 and M3 change no manifest.
Constraints:
- No calls to the real `herdr` binary in this milestone.
- No async runtime. The trait is usable from plain synchronous code.
- Global instructions apply (only this milestone changes manifests and `Cargo.lock`).
Interfaces:
- The trait, the status type and the error type defined here are the contract. M2 implements the trait for the herdr CLI. M3 uses only send-prompt and agent-status, plus the error kinds. Both use the in-memory fake in their tests.
- Crate layout for the later milestones:
  - M2 owns a CLI-backend module in this crate and a fake `herdr` executable target, which this milestone declares so M2 changes no manifest.
  - M3 owns a delivery module in this crate.
  - This crate may depend on `orch-core` (store, errors) and `orch-config` (tunables, agent registry).
- No runtime-event subscription in the trait for now (see Notes from user).
Context:
- Existing crates: `crates/orch` (binary), `crates/orch-core` (`error`, `paths`, `logging`, `task`, `store`, `lease`), `crates/orch-config` (`agents`, `models`, `tunables`).
- Workspace dependencies are declared in the root `Cargo.toml` `[workspace.dependencies]`. `serde_json`, `thiserror`, `tracing` and `tempfile` already exist there.
- `orch-config/src/tunables.rs` already has `herdr_timeout_s` (`ORCH_HERDR_TIMEOUT_S`, default 30) and `herdr` (the `HERDR` binary path, default `herdr`).
- `orch-config/src/agents.rs` has `herdr_detect` per agent kind.
- Python's `@pane:<id>` workers (agents herdr does not recognise) are prompted with `pane send-text` and then `pane send-keys <pane> enter`, and are watched only through their done file and time budget.
- herdr 0.9.5 statuses are `idle`, `working`, `blocked`, `done` and `unknown` (see `herdr --skill` and `herdr api schema --json`, the `AgentStatus` enum). "Not found" comes back as an error (`agent_not_found`).
- Socket protocol 22 offers `events.subscribe`, `events.wait`, `agent.wait` and a `pane_agent_status_changed` subscription event. The CLI has `herdr agent wait`.
Out of scope:
- The real herdr CLI backend and the fake executable (M2), deliveries and the store migration (M3), any `orch` command.
Open questions:
- none
Notes from user:
- Runtime events: option (a). CLI only in this phase: status comes from `agent get`, and the trait documents runtime events as a later addition. The socket subscription (`events.subscribe`) comes in Phase 6 with the event manager. (from: lead, the recommended option; the developer said go without choosing another)

## [x] M2: Herdr CLI backend, fake herdr executable, `orch lead check`
Depends on: M1 (the trait, types and error kinds it implements; the crate and target it fills)
Why separate: it runs in parallel with M3 once the contract exists. They change disjoint files, and each is a real piece of work.
Prefer: any
Tier: medium
Needs: coding, tool-use
Model why: well-specified implementation of a fixed trait against a documented CLI. Most of the work is careful subprocess handling, JSON parsing and tests, with little design.
Review: yes
Likely files: crates/orch-herdr/src/cli*, crates/orch-herdr/src/bin/, crates/orch-herdr/tests/, crates/orch/src/commands.rs, crates/orch/tests/cli.rs
Goal:
- The adapter trait is implemented by running the real `herdr` binary, for every herdr call listed in §4, with typed parsing of its JSON output and the error kinds from M1.
- A fake `herdr` executable makes the tests run through real subprocess calls.
- `orch lead check` works.
Why: this is the backend every later phase uses to reach agents, and §4 requires a fake herdr so the suite never needs a real one.
Acceptance criteria:
- Each trait method runs the matching command from the §4 table: `agent list`, `agent get`, `agent prompt`, `agent read --source recent-unwrapped --lines N`, `agent rename`, `pane rename`, `pane current`, `pane get`, `pane list`, `pane split --cwd`, `pane run`, `pane send-text`, `pane send-keys`, plus a workspace list. Flags and JSON fields are checked against herdr 0.9.5's help, skill text or schema.
- Every call is bounded by `ORCH_HERDR_TIMEOUT_S`. A herdr that hangs past it returns the timed-out error and leaves no child process behind. A test shows this with a fake that sleeps.
- The herdr binary comes from the `HERDR` tunable.
- Each of these failures returns the matching M1 error kind:
  - a missing binary;
  - a non-zero exit;
  - `agent_not_found`;
  - `agent_blocked`;
  - an unknown pane id.
  The status is read only from the status field of herdr's JSON, so a name or path that contains "working" or "blocked" is never misread (as Python's `herdr_state` guarantees).
- Prompting an `@pane:<id>` agent sends the text and then Enter to that pane. A failure there is reported as a stale pane, with the pane id in the message.
- A fake `herdr` executable is built by the workspace. It keeps its state in a directory given by an environment variable, answers in herdr 0.9.5's JSON shapes, and can simulate:
  - not found;
  - blocked;
  - a slow response;
  - a non-zero exit.
  It records every prompt and text it receives. Integration tests run the CLI backend against it for every trait method.
- `orch lead check` prints `OK` and exits 0 when it runs inside a herdr pane (`HERDR_ENV=1`, or `herdr pane current` succeeds). Otherwise it prints how to fix it and exits non-zero, as Python v4.10.1 does. This is tested with the fake herdr.
- The test suite gives the same result inside and outside a real herdr pane: tests never pick up the developer's `HERDR_*` environment or real socket.
Constraints:
- Do not change any `Cargo.toml` or `Cargo.lock` (M1 has made room).
- Read-only `herdr` commands (`--help`, `--skill`, `api schema --json`, `agent list`, `pane current`) may be run against the real herdr to learn the JSON shapes. Never prompt, rename, split or close real panes.
- No socket client (M1's decision: CLI only in this phase).
Interfaces:
- Implements M1's trait unchanged. If the trait truly needs to change, ask with a QUESTION instead of changing it (M3 codes against it in parallel).
Context:
- The herdr 0.9.5 JSON envelope is `{"id": "cli:agent:list", "result": {...}, ...}`.
  - Agents carry `name`, `pane_id`, `agent`, `agent_status`, `cwd`, `workspace_id`, `tab_id` and `completion_seq`.
  - Errors carry `{"error": {"code", "message"}}`, for example `agent_not_found`.
  Check these against `herdr api schema --json` (the `success_response` and `error_response` schemas).
- `herdr agent prompt` reports success after the text and Enter are written. It refuses with `agent_blocked` when the agent sits at an approval dialog (`herdr --skill`).
- Python's wrapper is `_herdr_run` and `herdr_state` in `~/.local/share/herdr-orchestra/v4.10.1/bin/orch` (about lines 357–410). Its `lead check` is `cmd_lead` (search for `"check"`).
- The `lead check` and `lead whoami` command names already exist as stubs in `crates/orch/src/commands.rs`. CLI integration tests live in `crates/orch/tests/cli.rs`.
- The old fake, `/mnt/workspace/projects/herdr-orchestra/tests/sim/herdr`, shows the idea: state files per agent, and a prompts log.
Out of scope:
- `orch lead whoami`, `team up` or any other command besides `lead check`.
- Deliveries (M3).
- Polling or event loops.
Open questions:
- none
Notes from user:

## [x] M3: Crash-safe prompt delivery
Depends on: M1 (the trait's send-prompt and status, and the error kinds; the in-memory fake for tests)
Why separate: it runs in parallel with M2 once the contract exists. It touches the store and a delivery module, not the CLI backend, and its correctness rules (record → send → confirm, same agent after a crash) deserve their own review.
Prefer: claude-1
Tier: high
Needs: reasoning, coding
Model why: crash-safety and a schema migration. The hard part is reasoning about every crash point between record, send and confirm, so that a prompt is never lost, never sent to a second agent, and never silently sent twice.
Review: yes
Likely files: crates/orch-core/migrations/, crates/orch-core/src/store.rs, crates/orch-herdr/src/delivery*, crates/orch-herdr/tests/delivery*
Goal: sending a prompt to an agent is crash-safe, as §4, §10, §15 and §16 require:
- the prompt is recorded durably before it is sent;
- it is marked delivered when herdr confirms the send;
- an unconfirmed prompt found after a restart is either confirmed from the agent's status or re-sent to the same agent;
- delivery failures are tracked per agent.
Why:
- Preserved Python safety (§0 item 8, 4.6.0): a crash between "decided to send" and "sent" must not lose the job or hand it to a second coder.
- §10: a task is never assigned to two workers at once.
Acceptance criteria:
- A new numbered migration adds the storage for pending prompt deliveries (§16), and the store applies it on open. An existing schema-1 database upgrades in place, and a test shows this.
- A delivery records at least:
  - the agent;
  - the task;
  - the attempt;
  - the prompt text;
  - its kind (coder or reviewer);
  - when it was recorded;
  - whether it is confirmed.
  The record is committed before anything is sent to the runtime.
- On a successful send, the delivery is marked confirmed. When the runtime returns an error, the delivery is not confirmed, the error kind and message are kept, and the agent's consecutive-failure count goes up. A confirmed send resets that count.
- Recovering unconfirmed deliveries after a restart. For a watchable agent, after a grace period of at least `ORCH_CONFIRM_S` if that tunable exists, otherwise a documented constant:
  - working or blocked: confirmed without re-sending;
  - not found: reported as gone, not re-sent;
  - otherwise: reported as unconfirmed and re-sendable.
  For an agent that is not watchable, the delivery is reported as unconfirmed. It is never re-sent automatically. Tests cover each branch with the in-memory fake.
- A re-send of an unconfirmed delivery always goes to the agent it was recorded for, with the same prompt, task and attempt. No API exists to re-send it to a different agent.
- Crash tests simulate a stop at each point:
  - after record, before send;
  - after send, before confirm.
  They do this by reopening the store and running recovery, and show that the prompt is neither lost nor confirmed twice.
- The delivery counter that decides benching after 3 consecutive failures (§15) can be read per agent. Benching itself is NOT implemented here.
Constraints:
- Do not change any `Cargo.toml` or `Cargo.lock`.
- Use only the trait from M1, never the CLI backend directly. Tests use the in-memory fake only.
- Add the migration as a new numbered file registered after `0001_init.sql`. Never edit `0001_init.sql`.
- No scheduler, no retry timer or loop, no benching and no lead wake-up. This milestone only provides the operations and their reports, which Phase 6 will call.
Interfaces:
- Uses M1's trait (send prompt, agent status), its status type including "not watchable", and its error kinds. If the trait truly needs a change, ask with a QUESTION instead (M2 implements it in parallel).
Context:
- The store is in `crates/orch-core/src/store.rs`. Migrations are `(version, include_str!(file))` pairs in `MIGRATIONS`, tracked with `PRAGMA user_version`. The module doc says later phases add tables, including "deliveries", as new numbered files. The current schema is `crates/orch-core/migrations/0001_init.sql` (tasks, task_deps, history, events).
- Python's model:
  - `t["wait"]` holds `agent`, `prompt`, `kind`, `file`, `attempt`, `since` and `delivering: True`, and is saved before `deliver()` (about line 1612).
  - After a successful send it sets `delivering = False`.
  - The poll loop (about line 1878) treats a `delivering` wait older than `CONFIRM_S` like this: herdr says working or blocked → confirmed; not found → `GONE`; otherwise → `UNCONFIRMED`.
  - `cmd_resend` (about line 1795) re-sends to the same agent.
- The tunables live in `crates/orch-config/src/tunables.rs`. Check whether a confirm-grace tunable exists before inventing one.
Out of scope:
- The herdr CLI backend (M2).
- The `orch resend` command.
- Scheduling, retries after 2 minutes, benching, and lead events (Phase 6).
Open questions:
- none
Notes from user:

## [x] M4: Fix for M2: orch lead check must reach herdr through the orch-herdr Runt...
Depends on: M2 (a fix to what M2 built)
Why separate: a change the developer asked for after M2 landed.
Review: yes
Likely files: crates/orch-herdr/src/cli*, crates/orch-herdr/src/bin/, crates/orch-herdr/tests/, crates/orch/src/commands.rs, crates/orch/tests/cli.rs
Tier: medium
Model why: a fix to M2, for the coder who built it.
Goal: orch lead check must reach herdr through the orch-herdr RuntimeAdapter (the CLI backend), not through its own herdr subprocess call: today it is special-cased in crates/orch/src/main.rs (outside M2's Likely files) and runs its own `herdr pane current` in crates/orch/src/commands.rs with a fixed timeout instead of ORCH_HERDR_TIMEOUT_S. Why: every later phase (team up, lead whoami, the event manager) must talk to herdr only through the adapter, so it stays replaceable and testable with the fakes. Done when: lead check behaves and prints exactly as today (OK inside a pane, the herdr-reachable-but-no-HERDR_ENV case, the 'Herdr not detected' help with exit 2), honours ORCH_HERDR_TIMEOUT_S and HERDR, is dispatched like the other commands instead of a special case in main.rs, and its tests still run against the fake herdr.
Why: the developer asked for this after M2 landed.
Acceptance criteria:
- orch lead check must reach herdr through the orch-herdr RuntimeAdapter (the CLI backend), not through its own herdr subprocess call: today it is special-cased in crates/orch/src/main.rs (outside M2's Likely files) and runs its own `herdr pane current` in crates/orch/src/commands.rs with a fixed timeout instead of ORCH_HERDR_TIMEOUT_S. Why: every later phase (team up, lead whoami, the event manager) must talk to herdr only through the adapter, so it stays replaceable and testable with the fakes. Done when: lead check behaves and prints exactly as today (OK inside a pane, the herdr-reachable-but-no-HERDR_ENV case, the 'Herdr not detected' help with exit 2), honours ORCH_HERDR_TIMEOUT_S and HERDR, is dispatched like the other commands instead of a special case in main.rs, and its tests still run against the fake herdr.
- nothing else that M2 built changes, and the check command passes
Open questions:
- none
Notes from user:
- Option (1): yes. Add the workspace-internal orch-herdr dependency to crates/orch/Cargo.toml plus the matching Cargo.lock change. That is the only manifest change allowed; no new external crates. (from: lead — the fix's goal requires orch to use RuntimeAdapter, and orch-herdr is a workspace crate, not a new third-party dependency; the 'only M1 changes manifests' rule exists to keep parallel milestones from colliding, and nothing runs in parallel with M4)

## [x] M5: Fix for M3: The schema upgrade test (schema1_database_upgrades_in_place)...
Depends on: M3 (a fix to what M3 built)
Why separate: a change the developer asked for after M3 landed.
Review: yes
Likely files: crates/orch-core/migrations/, crates/orch-core/src/store.rs, crates/orch-herdr/src/delivery*, crates/orch-herdr/tests/delivery*
Tier: high
Model why: a fix to M3, for the coder who built it.
Goal: The schema upgrade test (schema1_database_upgrades_in_place) upgrades an EMPTY schema-1 database, so it does not prove what §16 needs: that an existing run's data survives the upgrade. Done when a test opens a schema-1 database that already holds a task, its transition history and an event, upgrades it with this build, and shows those rows are unchanged and the new delivery tables are usable. Why: every later phase adds migrations, and this is the regression guard against an upgrade that loses a developer's run state. (The lead verified by hand that the current migration does keep the rows; only the test is missing.) Test-only change: no change to the migration or store behaviour.
Why: the developer asked for this after M3 landed.
Acceptance criteria:
- The schema upgrade test (schema1_database_upgrades_in_place) upgrades an EMPTY schema-1 database, so it does not prove what §16 needs: that an existing run's data survives the upgrade. Done when a test opens a schema-1 database that already holds a task, its transition history and an event, upgrades it with this build, and shows those rows are unchanged and the new delivery tables are usable. Why: every later phase adds migrations, and this is the regression guard against an upgrade that loses a developer's run state. (The lead verified by hand that the current migration does keep the rows; only the test is missing.) Test-only change: no change to the migration or store behaviour.
- nothing else that M3 built changes, and the check command passes
Open questions:
- none
