# Plan: Phase 4 — Agent and Model Management
Branch: feature/agent-model-management
Check command: cargo build --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
Setup command: cargo fetch
Isolation: worktree
Max parallel: 2
Scope: warn
Lead: lead (claude: opus)
Workers: claude-1 (claude: sonnet), opencode-1 (opencode: opencode/glm-5.3-flash-free)
Reviewer: reviewer (agy: gemini-3.8-flash)

## Global instructions
- The product spec is `docs/plan.md` in this repo. This run is §24 "Phase 4 — Agent and Model Management". The requirements are in:
  - §5 (agent registry, discovery, configuration, roles, team from one tool);
  - §5A (model catalog, tiers, deny policy);
  - §17 (CLI commands).
- The behaviour reference is the installed Python edition v4.10.1 at `~/.local/share/herdr-orchestra/v4.10.1/`: `bin/orch`, `agents.default.toml`, `models.default.toml`, `skills/`, `claude-commands/` and `opencode-commands/`. Do NOT use `/mnt/workspace/projects/herdr-orchestra`: that checkout is the old v4.4.0.
- Today's command output and exit codes are the contract. Keep the same command names, the same words where scripts or the playbooks read them (`OPENED`, `WAITING`, `FAILED`, `ok`, `BROKEN`, `skip`, `NEW`) and the same exit codes, unless a milestone says otherwise.
- Every call to herdr goes through the `orch-herdr` `RuntimeAdapter` (Phase 3). Nothing calls the `herdr` binary directly.
- Must not:
  - plan parsing or `orch sync` validation (Phase 5);
  - the scheduler or event manager (Phase 6);
  - git or worktree creation (Phase 7);
  - verification or review (Phase 8);
  - the GUI or REST API;
  - the installer.
  Commands outside this phase stay stubs.
- Done when `cargo build`, `cargo clippy` (warnings denied) and `cargo test` pass for the whole workspace.
- Tests never need a real agent CLI, a real herdr, network access or the developer's real home folder. They use fake executables on a temporary PATH, a temporary HOME or config directory, and the Phase 3 fakes for herdr. Running the suite inside or outside herdr gives the same result.
- Never read, print or store credential contents (tokens, API keys, auth files). Authentication checks only ask the tool, or check whether something exists.
- Stable Rust, edition 2024, no async runtime. Only M1 changes manifests (`Cargo.toml`, `Cargo.lock`, workspace members). If a later milestone truly needs another crate, ask with a QUESTION.

## Feature breakdown
- Registry, roles and launch configuration:
  - extend the agent entry with the §5 configuration fields that the Python registry lacks (display name, extra arguments, environment variables, working folder, capabilities, max concurrent sessions, timeout, cooldown, custom instructions), all optional, plus the `version_cmd` and `help_cmd` keys Python already reads;
  - the six roles of §5 (lead, coder, reviewer, tester, researcher, specialist);
  - the launch-command builder with the model policy (`orch agents launch`);
  - the read-only listing commands: `orch agents`, `orch models`, `orch models show|check`.
- Discovery and readiness:
  - per kind: installed on PATH, version, launch flags still present in `--help`, authentication status (new), skills installed, herdr detection;
  - `orch doctor [kind...]`;
  - skill distribution with `orch agents sync` and `orch agents check`, from skill files shipped inside the binary.
- Team from one tool:
  - `orch team <kind> [--coders N] [--models a,b] [--no-reviewer]` picks models from the catalog;
  - the logic behind `orch team up` opens a pane for each missing agent through the adapter, in the agent's folder, on its model, names it, and reports `OPENED` / `WAITING` / `FAILED`.
- How they connect: discovery and team both build on the registry, roles and launch builder. M1 is the foundation. It also owns every shared hotspot file: manifests, the command dispatch in `crates/orch/src/commands.rs` and `main.rs`, and library module lists. Discovery (M2) and team (M3) then change disjoint files and run in parallel on the two coders. That makes three milestones.

## [x] M1: Registry, roles, launch configuration and the listing commands
Depends on: none
Why separate: discovery (M2) and team (M3) both need the extended registry, the role type and the launch-command builder, and both would otherwise edit the same command dispatch and manifests. Landing this first lets M2 and M3 run in parallel without touching shared files.
Prefer: claude-1
Tier: high
Needs: architecture, coding
Model why: it fixes the registry schema and the role and launch types that every later phase reads, plus the command scaffolding two parallel milestones plug into. A wrong shape here costs both of them.
Review: yes
Likely files: Cargo.toml, Cargo.lock, crates/orch-config/, crates/orch/src/, crates/orch/tests/cli.rs, crates/orch/Cargo.toml
Goal:
- The agent registry carries the §5 agent configuration, with all new fields optional so today's TOML files keep loading.
- Roles are a typed set of the six §5 roles.
- One function builds an agent's launch command, on a given model, under the deny policy.
- `orch agents`, `orch agents launch <kind> [--model <id>]`, `orch models`, `orch models show <id>` and `orch models check <id>` work as in Python v4.10.1.
- The command scaffolding for M2 and M3 is in place.
Why:
- §5 asks for richer agent configuration and more roles.
- The developer and the lead use these listing commands at every planning step (`orch agents`, `orch models`).
- Later phases build on one launch builder.
Acceptance criteria:
- Today's shipped `agents.default.toml` and a user `agents.toml` written for Python v4.10.1 both load unchanged, merged key by key as now.
- The registry accepts these optional per-kind keys and validates their types: display name, extra launch arguments, environment variables, working folder, capabilities, max concurrent sessions, execution timeout, cooldown, custom instructions, `version_cmd` and `help_cmd`. A wrong type is an error that names the file and the key.
- Roles are one of lead, coder, reviewer, tester, researcher or specialist. An unknown role is an error that names the file and the key.
- The launch builder has the following behaviour, each case unit-tested:
  - it returns the registry's launch command, plus the model flag and the shell-quoted model when a model is given;
  - it refuses a model the policy denies, naming the matching pattern;
  - it refuses a model for a kind without `model_flag`, unless the caller allows the default model (Python's `strict=False`);
  - it warns when the catalog lists the model for other kinds only;
  - it adds the configured extra arguments.
- `orch agents launch <kind> [--model <id>]` prints the command. For an unknown kind, a denied model or a missing model flag it fails with Python's message and a non-zero exit.
- `orch agents` prints the registry table (kind, installed or not, roles, herdr detection, launch command), as Python's `cmd_agents` does with no subcommand.
- `orch models` prints the catalog table and the denied patterns. `orch models show <id>` prints one model's ratings and evidence. `orch models check <id>` exits 0 for an allowed model and non-zero for a denied one, as Python's `cmd_models` does.
- `orch agents check`, `orch agents sync`, `orch doctor` and `orch team` (including `team up`) are routed to their own handlers, which M2 and M3 fill in. Until then they keep the "not implemented" exit.
- CLI integration tests cover `agents`, `agents launch`, `models`, `models show` and `models check`, using a temporary config directory and PATH.
Constraints:
- Global instructions apply. This milestone is the only one that changes manifests and the shared dispatch files.
- No herdr calls, no running agent CLIs (except checking whether a binary is on PATH), no copying of skill files.
Interfaces:
- M2 and M3 use, unchanged:
  - the extended registry type and the role type;
  - the launch builder;
  - the "is this kind installed" check.
  If one of them truly needs a change, it asks with a QUESTION.
- The command handlers for `agents check`, `agents sync` and `doctor` live in a module that only M2 edits. The handlers for `team` and `team up` live in a module that only M3 edits. This milestone creates both modules with stub handlers, and any library modules M2 and M3 need, so that neither of them edits `commands.rs`, `main.rs`, a `lib.rs` module list or a manifest.
- `orch-config` already has the catalog: `models::load`, `Catalog::denied`, `model_tier` and `invalid_models`. Build on it rather than duplicating it.
Context:
- The registry today is `crates/orch-config/src/agents.rs` (`AgentKind`, `Registry`, `load`, `registry`), with defaults in `crates/orch-config/agents.default.toml`. The catalog is `crates/orch-config/src/models.rs`, with `models.default.toml`.
- The command names and their help lines are in `crates/orch/src/commands.rs`. `main.rs` dispatches them, and only `version` and `lead check` are real today.
- Python references in `~/.local/share/herdr-orchestra/v4.10.1/bin/orch`:
  - `cmd_agents` (about line 3826);
  - `launch_cmd` (about line 3381);
  - `cmd_models` (about line 549).
  Python runs `agents`, `doctor`, `models` and `team` (but not `team up`) outside a git repo too, through `NO_REPO_COMMANDS` at the bottom of the file.
- Exit codes are in `crates/orch-core/src/error.rs`.
Out of scope:
- Discovery, `doctor`, authentication checks and skill distribution (M2).
- `team` and `team up` (M3).
- Plan-level role rules such as "the lead is never a worker" (Phase 5 plan validation).
Open questions:
- none
Notes from user:

## [x] M2: Discovery, doctor, authentication status and skill distribution
Depends on: M1 (the extended registry, `version_cmd` and `help_cmd`, and the module stubs for its commands)
Why separate: it runs in parallel with M3 once M1 has landed, it touches different files, and each is a real piece of work.
Prefer: any
Tier: medium
Needs: coding, tool-use
Model why: well-specified readiness checks that follow Python's `doctor_kind` and `agents_sync`, plus the new authentication probe. The work is careful subprocess handling, file copying and tests, with little design.
Review: yes
Likely files: crates/orch-config/src/discovery.rs, crates/orch-config/src/discovery/, crates/orch-config/assets/, crates/orch/src/doctor.rs, crates/orch/src/doctor/, crates/orch/tests/agents*
Goal: orch can tell, for every registered agent kind, whether it is ready to use:
- installed on PATH;
- its version;
- every launch flag still present in the tool's help;
- authentication status;
- the orch skills installed;
- herdr detection.
`orch doctor [kind...]`, `orch agents check` and `orch agents sync` work as in Python v4.10.1, with authentication added.
Why:
- §5 "Agent discovery", steps 1–9. For example, codex-cli 0.167 dropped `--full-auto`, and `doctor` is how the developer finds such breaks.
- §5 also says that an installed CLI does not mean an authenticated account.
Acceptance criteria:
- For each kind, discovery reports:
  - installed or not (binary on PATH);
  - the version from `version_cmd`, default `<binary> --version`; "unknown" when it fails;
  - for every option in the launch command, whether it appears in `help_cmd` output (default `<binary> --help`);
  - authentication status;
  - whether the orch skills and commands are installed in the kind's folders;
  - whether herdr lists the kind, taken through the adapter, with "not checked" when herdr is not reachable.
  Each external call has a timeout.
- Authentication status is one of authenticated, not authenticated or unknown, with a short reason. It uses only a non-destructive mechanism each tool documents, or the presence of its login state, and never reads or prints credential contents. Kinds without such a mechanism report unknown. Which mechanism each shipped kind uses is configurable in the registry, documented in the shipped `agents.default.toml` and checked against each tool's real help.
- `orch doctor [kind...]` prints per kind `ok`, `BROKEN` or `skip` (not installed), with one `ok` or `BAD` line per check, and exits 1 if any kind is broken, as Python's `cmd_doctor` does. An unknown authentication status is not counted as broken.
- `orch agents sync [kind...]` copies the orch-plan and orch-run skills and the slash commands into:
  - the shared folders (`~/.agents/skills`, and the OpenCode command folders);
  - every installed kind's `skills_dirs` and `commands_dirs`, in its `commands_from` flavour.
  It prints Python's status table (`NEW` or `ok`, the folders, the invoke hint) and the "synced N row(s)" line, is idempotent, and records every written path in the uninstall manifest Python uses.
- `orch agents check` reports, per installed kind, whether its skills and commands are installed and current, and tells the developer to run `orch agents sync` when not.
- The skill and command files are shipped inside the binary. Their text is the same as Python v4.10.1's `skills/`, `claude-commands/` and `opencode-commands/`.
- Tests use fake agent executables on a temporary PATH and a temporary HOME. They cover:
  - a missing binary;
  - a failing version command;
  - a launch flag that is missing from the help;
  - each authentication outcome;
  - sync running twice (the second run reports nothing new);
  - check before and after a sync;
  - herdr not reachable.
Constraints:
- Do not change any manifest, `commands.rs`, `main.rs` or a `lib.rs` module list (M1 made room). If that turns out to be impossible, ask with a QUESTION.
- Herdr only through the `RuntimeAdapter`.
- Never touch real `~/.claude`, `~/.codex` or other real folders in tests.
Interfaces:
- Uses M1's registry, roles, launch builder and "installed" check unchanged. M3 runs in parallel and does not use anything from this milestone.
Context:
- M1 (commit `10dd7a1`) created this milestone's stubs: `crates/orch-config/src/discovery.rs` (library) and `crates/orch/src/doctor.rs` (handlers for `doctor`, `agents check` and `agents sync`). The skill-distribution code also belongs under these two files. Submodules declared from inside them don't need a `lib.rs` change.
- Python's references:
  - `doctor_kind` and `cmd_doctor` (about lines 3900–3960);
  - `agents_sync`, `sync_skills`, `sync_commands` and `manifest_record` (about line 3792 and the helpers above it);
  - `agent_assets` (which skill and command files each kind should have).
  The uninstall manifest is `~/.local/share/herdr-orchestra/manifest.txt`.
- The skill files are in `~/.local/share/herdr-orchestra/v4.10.1/skills/orch-plan`, `skills/orch-run`, `claude-commands/` and `opencode-commands/`.
- Shipped kinds: claude, codex, opencode, amp, hermes and agy (`crates/orch-config/agents.default.toml`). Real binaries on this machine may be used to learn their `--help` and authentication commands. Never log in, log out or change settings.
- The herdr agent kinds that herdr can detect are printed by `herdr agent` (Python checks the kind name in that output). Reach it through the adapter, or say in a QUESTION if the adapter lacks it.
Out of scope:
- Registry fields and the launch builder (M1).
- `team` and `team up` (M3).
- Storing credentials, logging agents in, the installer.
Open questions:
- none
Notes from user:
- Option (1): add a method to the RuntimeAdapter trait that returns the agent kinds herdr can detect, and implement it in the CLI backend, the in-memory fake and the fake herdr executable. You may change crates/orch-herdr/ (adapter.rs, cli.rs, fake.rs, bin/fake_herdr.rs and their tests) for this, and nothing else in that crate. M3 only uses the trait and implements nothing, so this does not collide with it. Source: in herdr 0.9.5 the plain `herdr agent` help output ends with a line 'kinds: pi|claude|codex|gemini|...|agy|...|opencode|...|amp|...|hermes|...' (checked on this machine), which is what Python matches the kind name against. When herdr is not reachable, doctor reports 'not checked', as Python does. Do not use `herdr agent list` for this: it only shows live agents. (from: lead: the plan requires every herdr call to go through the adapter; the kinds line was checked against the real herdr 0.9.5)

## [x] M3: Team from one tool: `orch team <kind>` and the `team up` engine
Depends on: M1 (the registry, roles, launch builder and the stub module for the team commands)
Why separate: it runs in parallel with M2 once M1 has landed. It touches different files, and its pane-opening logic deserves its own review.
Prefer: any
Tier: medium
Needs: coding, reasoning
Model why: `team` is a pure picker over the catalog, and `team up` is a clear sequence over the Phase 3 adapter. The care needed is in idempotence and the `WAITING` / `FAILED` reporting, not in design.
Review: yes
Likely files: crates/orch-config/src/team.rs, crates/orch-config/src/team/, crates/orch/src/team.rs, crates/orch/src/team/, crates/orch/tests/team*
Goal:
- `orch team <kind> [--coders N] [--models a,b] [--no-reviewer]` prints the `Workers:` and `Reviewer:` lines for a team built from one installed tool, as Python v4.10.1 does.
- The `team up` engine opens a herdr pane for every agent in a given team that herdr cannot find, through the `RuntimeAdapter`. The pane is in that agent's folder, running the tool on its model, and named as in the team.
Why: §5 "Team from one tool (no solo mode)", preserving 4.10.0. A developer with one tool or one window still gets a full team, and the lead builds it by itself.
Acceptance criteria:
- `orch team <kind>` behaves as Python's `cmd_team`:
  - it picks coder models spread over the catalog's allowed tiers for that kind: two by default, the weakest and the strongest; with `--coders N`, the strongest first, then the weakest, then the middle;
  - the reviewer is on the strongest model;
  - `--models a,b` overrides the pick, and a denied model is refused naming its pattern;
  - it warns when the kind has no allowed high-tier model, and when its binary is not on PATH;
  - it refuses a kind without the coder role, and an unknown kind;
  - it prints the `TEAM from` table and the plan's `Workers:` and `Reviewer:` lines, written as `name (kind: model)`, or `name (kind)` when the kind has no model flag.
  - Unit tests cover each case with a test catalog.
- The `team up` engine takes a team: for each agent its name, kind, model and folder, plus the pane to split from. For every agent herdr cannot find, it:
  - splits a pane from the given pane, in the agent's folder;
  - runs the M1 launch command for its kind and model;
  - names the pane's agent.
  Agents herdr already finds are left alone, so running it twice opens nothing new.
- `team up` reports per agent:
  - `ok` (already running);
  - `OPENED` (with the pane id and command);
  - `WAITING` (the new agent did not become ready within `ORCH_TEAM_WAIT_S`, default 90 s, for example a first-run "trust this folder" question; it tells the developer which pane to look at);
  - `FAILED` (herdr could not open the pane, with herdr's message).
  It ends with Python's `TEAM UP: done` line. With dry-run, it prints `WOULD` lines and changes nothing.
- Engine tests use the Phase 3 in-memory fake, and cover: all running; some missing; a pane that stays blocked (`WAITING`); a split failure (`FAILED`); dry-run; and a second run after success.
Constraints:
- Do not change any manifest, `commands.rs`, `main.rs` or a `lib.rs` module list (M1 made room). If that turns out to be impossible, ask with a QUESTION.
- Herdr only through the `RuntimeAdapter`.
- `ORCH_TEAM_WAIT_S` is a tunable read like the others in `crates/orch-config/src/tunables.rs`. If adding it there would change a file M2 also edits, ask with a QUESTION.
Interfaces:
- Uses M1's registry, roles and launch builder unchanged. M2 runs in parallel and shares nothing with this milestone.
- The input to the `team up` engine (a team as a list of name, kind, model and folder, plus the pane to split from) is what Phase 5 will build from the parsed plan. The `orch team up` command itself stays a stub in this phase (see Notes from user).
Context:
- M1 (commit `10dd7a1`) created this milestone's stubs: `crates/orch-config/src/team.rs` (library) and `crates/orch/src/team.rs` (handlers for `team` and `team up`). The launch builder is `crates/orch-config/src/launch.rs`.
- Python's references are in `~/.local/share/herdr-orchestra/v4.10.1/bin/orch`:
  - `team_models`, `team_pick` and `cmd_team` (about lines 3405–3490);
  - `team_up` (about line 3504; it reads the run's state for workers, folders and models);
  - `TEAM_WAIT_S`.
- The adapter is `crates/orch-herdr/src/adapter.rs`. It has `split_pane` (with `SplitPane::right(from, cwd)`), `run_command`, `rename_agent`, `agent` / `agent_status` and `current_pane`, and `fake.rs` is the in-memory fake.
- §5 example: one Codex pane leads to `orch team codex`, which gives `codex-1 (codex: gpt-6-luna), codex-2 (codex: gpt-6-sol)` and a reviewer on `gpt-6-sol`.
Out of scope:
- Discovery and doctor (M2).
- Plan parsing, `orch sync` and `orch start` (Phases 5 and 7).
Open questions:
- none
Notes from user:
- `orch team up` on the command line: option (a). Build and test the engine now, and leave the `orch team up` command as a stub (the "not implemented" exit) until Phase 5 can hand it the plan's team. (from: the developer)

## [x] M4: Fix for M2: The test suite is flaky: discovery::tests::probe_reports_exi...
Depends on: M2 (a fix to what M2 built)
Why separate: a change the developer asked for after M2 landed.
Review: yes
Likely files: crates/orch-config/src/discovery.rs, crates/orch-config/src/discovery/, crates/orch-config/assets/, crates/orch/src/doctor.rs, crates/orch/src/doctor/, crates/orch/tests/agents*
Tier: medium
Model why: a fix to M2, for the coder who built it.
Goal: The test suite is flaky: discovery::tests::probe_reports_exit_output_and_missing_binaries fails about 1 run in 20 with Failed("Text file busy (os error 26)") instead of running the fake tool. Cause: tests write a fake executable script and exec it at once while other test threads fork, so a child can inherit the script's open write handle (ETXTBSY on Linux). This made the final check of `orch finish` fail. Done when: every test helper that creates a fake executable (in crates/orch-config and crates/orch tests, wherever scripts are written then run) can no longer hit ETXTBSY, and `cargo test -p orch-config --lib` plus the orch integration tests pass 50 runs in a row; no change to product behaviour. Why: a flaky check blocks merges and finishes for every later phase.
Why: the developer asked for this after M2 landed.
Acceptance criteria:
- The test suite is flaky: discovery::tests::probe_reports_exit_output_and_missing_binaries fails about 1 run in 20 with Failed("Text file busy (os error 26)") instead of running the fake tool. Cause: tests write a fake executable script and exec it at once while other test threads fork, so a child can inherit the script's open write handle (ETXTBSY on Linux). This made the final check of `orch finish` fail. Done when: every test helper that creates a fake executable (in crates/orch-config and crates/orch tests, wherever scripts are written then run) can no longer hit ETXTBSY, and `cargo test -p orch-config --lib` plus the orch integration tests pass 50 runs in a row; no change to product behaviour. Why: a flaky check blocks merges and finishes for every later phase.
- nothing else that M2 built changes, and the check command passes
Open questions:
- none
