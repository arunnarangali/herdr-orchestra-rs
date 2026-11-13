# Project: Herdr Orchestra — Rust Edition

> **Plan revision 2026-11-12.** Updated against `main` of `arunnarangali/herdr-orchestra` at **v4.10.0** (commit `4ac25de`), plus open PR #16 (v4.10.1, automatic releases). Section 0 lists what changed from the previous version of this plan and the decisions arun made. Everything after section 0 is the plan itself.

---

# 0. What changed in this revision, and why

The previous plan was written before 4.6–4.10. Several of its rules now disagree with how the Python `orch` really works, and several things the Python `orch` does well were missing. In short:

| # | Area | Before (old plan) | Now (this plan) | Why |
|---|---|---|---|---|
| 1 | Lead lifecycle (§8, §25, §26) | The lead plans and then finishes; it is called again only for a clarification. | The lead plans, dispatches and **pauses**. A non-AI event manager runs everything and **wakes the lead** only when a decision is needed (finished, question, failed, held, ready, all done). No AI runs between wake-ups. `Lead mode: off` keeps the old "scheduler alone" behaviour. | 4.7.0 made this the design, and a real run with real agents passed with it. **Decided: D1.** |
| 2 | Event coordinator (§9) | Generic event list, in-process bus. | Adds the **lead event queue**: events stored in the same write as the state change, the lead woken once per batch when idle, re-woken after 5 min (3 times), reading = acknowledging, blocking events stay open, ready work starts by itself after 15 min so a run never stalls on the lead. | 4.7.0 behaviour that must be preserved. |
| 3 | Model selection (new §5A) | Only "default model, if configurable". | Model **catalog** with tiers (small / medium / high), 1–5 ratings, a reasoning floor per tier, a **deny policy** for ultra-premium models (`*fable*`, `*astra*`, `best`), tier-aware dispatch, one escalation to a stronger model, and a planned-vs-actual model table in the report. | 4.7.0. |
| 4 | Milestone sizing (§8) | "Avoid excessive task splitting." | No fixed milestone counts at all. The lead writes a **Feature breakdown**, starts from one milestone, and splits only for a reason. Every milestone in a plan with 2+ milestones carries a `Why separate:` line. | 4.9.0. arun explicitly rejected number rules like "small = 1, medium = 2–3". |
| 5 | Post-summary fixes (§14, §17) | Only review fix jobs. | Adds `orch fix M1 "<change>"`: a change asked for after the summary goes **straight to the coder who built M1** as one small follow-up milestone, with no new planning round. | 4.8.0. |
| 6 | Single tool installed (§5, §7, §19) | "If only Codex is installed, it should work with Codex" (no how). | `orch team <tool>` builds a full team (coders + reviewer) from one tool, and `orch team up` asks Herdr to open the missing panes. The lead does this by itself when it is the only agent window. **No solo mode** (one agent doing everything through its own sub-agents). | 4.10.0. arun chose this over solo mode. |
| 7 | Git (§13) | "Independent worker branches." | **One** `feature/<slug>` branch per run. Coders work on **detached** worktrees; work in progress lives in hidden refs `refs/orch/<branch>/Mn` (never pushed). One real commit per milestone (the coder's `COMMIT:` line + `Milestone: Mn` trailer). `orch finish` pushes and opens a PR. | 4.5.0, arun's own request. Per-worker branches would bring back what he asked to remove. |
| 8 | Safety already in Python (§10, §13, §15) | Missing or generic. | Explicitly preserved: single-driver lease, crash-safe prompt delivery (record, then send, then confirm or re-send to the **same** agent), merge journal, check-before-commit, `integration-fix` rounds, risk hold + `orch approve`, scope check, bounded lost attempts, setup re-run when lockfiles change, red-baseline stop, run budget. | 4.6.0. "Do not silently remove existing capabilities." |
| 9 | Task states (§11) | Had `Ready`, `Assigned`; missing `NeedsFix`, `Held`. | Matches the real transition table (`needs-fix`, `held` added; `merged` terminal; `orch set` limited to safe states). `Ready`/`Assigned` are kept only as derived views, not stored states. | Real states are in `TRANSITIONS` in `bin/orch`. |
| 10 | Contracts (§12, §14) | Prose contract. | Keeps the 4.6 data contracts: `task.json` (generated from `plan.md`), `result-N.json` from coders, `review-N.json` from the reviewer, with the `COMMIT:` / `QUESTION:` / `BLOCKER:` line fallback. | `docs/contracts.md`. |
| 11 | Skills, playbooks, installer (§19, Phase 11) | Only "installation scripts". | Preserves the lead playbooks (`orch lead plan|run`), thin skill / slash-command wrappers for every agent, `orch agents sync`, side-by-side versions, `.orch-version` pinning, uninstall manifest, curl installer. | 4.3–4.4 behaviour developers already rely on. |
| 12 | Releases (Phase 11) | "Release packaging." | Every feature PR bumps `VERSION` and adds a CHANGELOG entry; a merge to `main` with a new `VERSION` **tags and publishes the release by itself** (no hand-pushed tags). The website (`site/`, Vercel) is updated in the same PR. | PR #16 (4.10.1) and arun's standing rules. |
| 13 | Known gaps (new §15A) | — | Lists the two open problems from the 4.7 real run so the Rust edition fixes them by design: Claude Code permission prompts stall unattended runs; an answer typed by the developer in their own terminal does not tell the lead. | Found in the 2026-11-12 real run, never fixed in Python. |
| 14 | CLI (§17) | New command names only. | Keeps every existing `orch` command and plan line, and marks which new names (`init`, `gui`, `pause`, …) are additions. Notes that the old plan's `orch start` and `orch retry` clash with existing meanings. | "Preserve and improve the existing `orch` command experience." |
| 15 | Main interface (§6B, §18, §21, Phase 9) | Interactive TUI (Ratatui + Crossterm). | A **GUI desktop app** (Tauri) on top of a **local REST API** served by the same Rust core. The CLI stays, because the lead agent runs `orch` commands in its Herdr pane. | arun asked on 2026-11-12 for a GUI app instead of a TUI. |

## Nothing from the original plan was dropped

Every section, list and rule of the original plan is still here (checked line by line on 2026-11-12). Only these items were changed on purpose, because the repo already decided them differently or because arun asked:

- "The Lead may finish after planning" → the lead pauses and is woken (row 1, decision D1).
- "Independent worker branches" → one feature branch + hidden refs (row 7).
- `Ready` and `Assigned` as stored states → derived views (row 9).
- `.orchestrate/worktrees/` → worktrees stay in `../<repo>-wt/` as today (§16).
- The TUI → a GUI desktop app on a local REST API (row 15). arun asked for this.

Everything the Python edition has today (every command, plan line, setting, file, installer flag and test tool) is listed in **Appendix A**, so the parity work can tick it off.

## Decisions (settled by arun on 2026-11-12)

**D1. Who starts the next ready step during a run? The lead does, as in 4.7.**
The lead dispatches (`orch dispatch M2 M3`) and pauses. The event manager wakes it when something lands. If the lead hasn't acted within 15 minutes, the work starts by itself. `Lead mode: off` means there is no lead in the run.
Example: M1 merges, the lead is woken, sees that M2 and M3 are now ready, runs `orch dispatch M2 M3`, and pauses.

**D2. The Rust `orch` is a drop-in replacement for the Python one.**
It keeps the same command names, the same `plan.md` format and the same `.orchestrate/` folder. On its first run it imports an in-flight `state.json`. It ships as **5.0.0**, and the Python edition stays available as 4.x for anyone who pins it.

**D3. The Rust code lives in its own repository, which arun creates.**
The Python repo (`herdr-orchestra`) is the reference for behaviour, docs and the website. Things the two repos share (the plan format, `agents.default.toml`, `models.default.toml`, the playbooks, `docs/contracts.md`) must stay compatible. Where the installer and release flow end up is settled once the new repo exists.

---

# 1. Project Vision

Build a complete, production-quality Rust implementation of Herdr Orchestra, based on the existing project:

https://github.com/arunnarangali/herdr-orchestra (Python, single `bin/orch` script, v4.10.0 at the time of writing)

The objective is to evolve the existing Python-based orchestration system into a modular, reliable, event-driven Rust application.

This is not simply a Python-to-Rust translation.

Build a complete developer-facing multi-agent software development orchestration platform that coordinates different AI coding agents, manages their execution, tracks dependencies, handles failures, verifies implementations, and integrates completed work.

The system must be simple enough for a developer to start using within minutes, while providing advanced configuration and control for experienced developers.

### Core philosophy

- Herdr provides the terminal and coding-agent runtime.
- Herdr Orchestra provides orchestration and workflow intelligence.
- AI agents perform software development tasks. The **lead** decides *what* and *why*; **coders** decide *how*; the **reviewer** reports problems only.
- The **event manager** (a plain program, no AI) sends work, watches agents, runs checks, reviews and merges, and wakes the lead only when a decision is needed. Between wake-ups no AI runs and nothing costs money.
- The scheduler manages task allocation and execution, following each milestone's model tier.
- Git provides isolation and integration: one feature branch per run, ending in one PR.
- Automated checks provide verification, and nothing red is ever committed.
- Humans provide decisions and approvals when necessary (questions only they can answer, held risky changes, the final merge).
- **Orchestra never calls an LLM API itself.** Every AI in the system is an agent CLI running in a Herdr pane, on the developer's own login.

The final product must be lightweight, modular, deterministic, extensible, and fault-tolerant.

---

# 2. First Step: Analyze the Existing Repository

Before writing code, thoroughly inspect:

https://github.com/arunnarangali/herdr-orchestra

Understand the complete existing implementation. The main sources are:

- `bin/orch` (about 4,000 lines of Python): every command, the state machine (`TRANSITIONS`, `move()`), autopilot / event manager, herdr calls, git flow.
- `playbooks/plan.md`, `playbooks/run.md`: what the lead is told to do (printed by `orch lead plan|run`).
- `skills/`, `claude-commands/`, `opencode-commands/`: thin wrappers that run the playbooks.
- `agents.default.toml`: the agent registry (claude, codex, amp, opencode, hermes, agy).
- `models.default.toml`: the model catalog, tiers and deny policy.
- `docs/contracts.md`: `task.json`, `result-N.json`, `review-N.json`.
- `install.sh`, `scripts/release.sh`, `.github/workflows/`: install, versions, releases.
- `tests/run.sh`, `tests/sim/` (fake herdr and fake agents), `tests/live/` (opt-in real run).
- `CHANGELOG.md` and `site/` (the website and docs at herdr-orchestra.vercel.app).

Analyze:

- Existing Python architecture.
- CLI commands (full list in §17).
- Lead planning workflow (research, Feature breakdown, milestones as issues, tiers, `Why separate:`).
- Lead run workflow (dispatch, pause, wake, decide; `Lead mode: off`).
- Coder execution workflow.
- Autopilot implementation: the event manager (`orch autopilot`), its event queue and lead wake-ups.
- Agent registry, `orch agents launch|sync|check`, `orch doctor`.
- Model catalog, tiers, deny policy, tier-aware dispatch, escalation.
- Team building from one tool (`orch team`, `orch team up`).
- Herdr integration (the exact herdr commands used, see §4).
- Task state machine and transition table.
- Dependency management, hotspot files, `Parallel: allow`.
- Git flow: one feature branch, detached worktrees, hidden refs, merge journal, `orch finish`.
- Verification pipeline: baseline check, check-before-commit, `integration-fix`.
- Reviewer workflow and review pipelining (`handoff`, fix jobs).
- Retry and recovery logic: fix/review/conflict rounds, lost-attempt budget, benching, crash-safe sends.
- Agent cooldown management.
- Human clarification handling (`QUESTION:` → lead → developer → `orch answer` → same coder).
- Post-summary fixes (`orch fix`).
- Risk hold, scope check, run budget.
- Configuration (plan header lines, `ORCH_*` tunables, registry and catalog overrides).
- Skills and playbooks.
- Existing tests.
- Installation and uninstallation, versions and pinning.
- Releases (automatic on merge, PR #16).
- Documentation and website.

Create a feature-parity document containing:

1. Existing functionality.
2. Functionality that must be preserved.
3. Functionality that should be redesigned.
4. Redundant functionality that should be removed.
5. Missing functionality that should be introduced.
6. Architectural improvements for the Rust implementation.

Do not start by blindly translating Python files into Rust modules.

Understand the behavior and responsibilities of each subsystem first.

Do not silently remove existing capabilities.

---

# 3. Fundamental Architecture

Build a Rust workspace with clearly separated modules.

The architecture must have the following main responsibilities:

1. Core orchestration engine.
2. Lead interface: planning playbook, run playbook, and the lead event queue.
3. Task and milestone management (plan parser, `plan.md` ↔ state sync).
4. Dependency engine.
5. Deterministic scheduler (tier-aware).
6. Independent event manager (the reworked autopilot).
7. Agent registry and management, including team building from one tool.
8. Model catalog and policy.
9. Herdr runtime integration.
10. Git and worktree management.
11. Verification and review.
12. Failure recovery.
13. Persistent state management.
14. CLI.
15. GUI app and the local REST API it uses.
16. Reporting and observability.
17. Installer, skill distribution and release tooling.

All interfaces must communicate through well-defined contracts.

Avoid unnecessary microservices.

Start with a modular, asynchronous Rust application.

**Single driver.** Exactly one process drives a run (the event manager). It holds a lease (`.orchestrate/driver.lock`, released by the kernel if the process dies). Every other process (CLI, GUI, REST API, the lead's commands) either reads, or sends a *request* the driver carries out (`dispatch`, `answer`, `fix`, `approve`). A second driver is refused.

---

# 4. Herdr Runtime Integration — Critical Requirement

Herdr is an existing Rust-based terminal multiplexer and AI coding-agent runtime.

Use the existing Herdr installation as the terminal and agent execution backend.

IMPORTANT:

Do not rebuild Herdr.

Do not create another terminal emulator.

Do not implement another terminal multiplexer.

Do not duplicate Herdr's workspace, tab, pane, shell process, agent detection, or session management capabilities.

Use Herdr's documented and supported integration interfaces.

Before implementing the integration, inspect the actual Herdr documentation, CLI help, socket API, protocol, and source where necessary.

Never assume undocumented command names, socket formats, or event payloads.

### What the Python edition uses today (start here)

The Python `orch` drives Herdr only through its CLI, with a timeout on every call (`ORCH_HERDR_TIMEOUT_S`, 30 s). Tested with herdr 0.9.5.

| Need | herdr command used today |
|---|---|
| List agents and their panes | `herdr agent list` |
| Agent status (idle / working / blocked / gone) | `herdr agent get <name>` (the `status` field) |
| Send a prompt | `herdr agent prompt <name> <text>` |
| Read recent output | `herdr agent read <name> --source recent-unwrapped --lines N` |
| Name an agent / pane | `herdr agent rename`, `herdr pane rename` |
| Which pane am I in | `herdr pane current` (fallback when `HERDR_ENV` doesn't reach the agent's shell, e.g. Hermes) |
| Pane details | `herdr pane get`, `herdr pane list` |
| Open a new agent pane (`orch team up`) | `herdr pane split --cwd <dir>`, then `herdr pane run <cmd>` |
| Agents herdr cannot detect (`@pane:<id>`) | `herdr pane send-text`, `herdr pane send-keys` |

Completion is detected from a **done file** the agent writes plus the herdr status, not from herdr callbacks. In Rust, first check whether Herdr now offers a socket API or event stream for status changes; use it if it is documented, and keep the CLI path as a fallback.

### Herdr responsibilities

Herdr owns:

- Workspace management.
- Tab management.
- Pane creation (including the panes `orch team up` asks for).
- Terminal execution.
- Shell process management.
- Agent process execution.
- Terminal input and output.
- Agent detection.
- Agent state information where supported.
- Session management.
- Terminal lifecycle.

### Orchestra responsibilities

Herdr Orchestra owns:

- Agent assignment and roles.
- Model tier per milestone and per agent.
- Task allocation.
- Execution scheduling.
- Dependency management.
- Task completion decisions.
- Verification.
- Review.
- Recovery.
- Git operations.
- Workflow state.
- The lead's event queue.
- Execution history.

### Build a dedicated Herdr integration adapter

The adapter should provide typed Rust interfaces for:

- Connecting to Herdr.
- Checking runtime availability (`orch lead check`: inside a herdr pane or not).
- Discovering workspaces.
- Creating or selecting workspaces.
- Creating agent execution panes (split from the lead's pane, in a given folder).
- Launching agents (with the registry's launch command and model flag).
- Naming agents.
- Sending prompts and input, with **delivery confirmation** (see §15).
- Reading terminal output.
- Monitoring agent status.
- Receiving runtime events.
- Detecting agent completion.
- Detecting agent failure.
- Detecting blocked agents (including permission prompts and first-run questions such as "trust this folder").
- Detecting stale pane ids (`STALE pane`).
- Closing or cleaning up sessions where appropriate.

Agents herdr cannot watch (`herdr_detect = false`) must keep working: they rely on the done file and the time budget only, are never reported GONE / BLOCKED / IDLE, and show as `detect:off`.

Use the actual capabilities exposed by Herdr.

If a capability is not exposed by Herdr, document the limitation and implement a safe alternative without duplicating its entire runtime.

Make the runtime adapter replaceable so alternative execution backends can be supported in the future. The test suite needs a **fake herdr** backend (the Python edition has `tests/sim/herdr`).

---

# 5. Agent Management System

The developer must be able to connect and manage multiple AI coding agents easily.

Support configurable agents including (all six are in today's registry):

- Claude Code (`claude`).
- OpenAI Codex (`codex`).
- OpenCode (`opencode`).
- Amp (`amp`).
- Hermes (`hermes`).
- Antigravity (`agy`).
- Other compatible CLI coding agents (any agent that can run shell commands).

**Any installed agent can play any role** (lead, coder, reviewer). Do not tightly couple the orchestration engine to individual providers.

### Agent registry

Keep the TOML registry format: defaults shipped with the binary (`agents.default.toml`), overrides in `~/.config/herdr-orchestra/agents.toml`, merged key by key. Keys used today: `binary`, `launch`, `model_flag`, `herdr_detect`, `roles`, `skills_dirs`, `version_cmd`, `help_cmd`, plus how to invoke the skills.

### Agent discovery

Implement automatic agent discovery.

When the application starts:

1. Scan configured executable locations and PATH.
2. Identify supported agent CLIs.
3. Check executable availability.
4. Detect installed versions where possible.
5. Check that every launch flag in the registry still exists in the tool's `--help` (`orch doctor` does this today; for example codex-cli 0.167 dropped `--full-auto`).
6. Validate that each agent can launch.
7. Check authentication status using supported non-destructive mechanisms (new; not in Python yet).
8. Check that the `orch-plan` / `orch-run` skills are installed for the agent (`orch agents check`), and offer `orch agents sync` when they are not.
9. Report whether the agent is ready for use.

Keep installation and authentication separate.

An installed CLI does not necessarily mean its account is authenticated.

Never request or store provider passwords or private API credentials unnecessarily.

Use existing provider authentication mechanisms and secure credential storage when API credentials are explicitly configured.

### Agent configuration

Each agent should support:

- Agent identifier.
- Provider (kind).
- Display name.
- Executable.
- Launch command.
- Arguments.
- Model flag and the model it runs (`claude-1 (claude: sonnet)`).
- Supported roles.
- Authentication status.
- Runtime status.
- Herdr detection on/off.
- Capabilities.
- Maximum concurrent sessions.
- Execution timeout.
- Cooldown.
- Custom environment variables.
- Working directory.
- Custom agent instructions.
- Skill folders.

### Agent roles

Support configurable roles:

- Lead.
- Coder.
- Reviewer.
- Tester.
- Researcher.
- Specialist.

Rules carried over from Python: the lead never codes or reviews in its own run (`orch sync` rejects a lead that is also a worker or the reviewer). The reviewer is never the coder of the change it reviews.

Allow multiple agents of the same provider.

For example:

- Claude 1: Coder (sonnet).
- Claude 2: Coder (haiku).
- Claude 3: Reviewer (opus).
- Codex 1: Coder (gpt-6-luna).
- OpenCode 1: Tester.

Do not assume one provider can only have one active agent.

### Team from one tool (no solo mode)

A developer with only one tool installed, or only one window open, still gets a full team. Preserve 4.10.0:

- `orch team <kind> [--coders N] [--models a,b] [--no-reviewer]` picks coders spread over the tiers the model catalog allows for that tool (two by default: the weakest and the strongest) and a reviewer on the strongest, and prints the plan's `Workers:` and `Reviewer:` lines. It warns when the tool has no allowed high-tier model (today Codex: `gpt-6-luna` small, `gpt-6-sol` medium, Astra denied).
- `orch team up [--dry-run]` (after start) asks **Herdr** to open a pane for every planned agent it can't find: split from the lead's pane, in that agent's folder (its worktree or the repo), running the tool on its plan model, named as in the plan. Agents already running are left alone, so it is safe to run again. It reports `WAITING` when a new pane stops at a first-run question.
- The lead does this by itself: when it sees no agent pane besides its own at planning time, it runs `orch team <its kind>`, writes the lines into the plan, and tells the developer how many windows will open.

Example:

```
you (1 Codex pane) ──► $orch-plan add a login page
                          lead: only me here → orch team codex
                          Workers: codex-1 (codex: gpt-6-luna), codex-2 (codex: gpt-6-sol)
                          Reviewer: reviewer (codex: gpt-6-sol)
               you ──► go
                          orch team up → 3 more Codex panes open → the run as usual
```

There is **no solo mode** (one agent doing all roles through its own sub-agents). It would break three core rules: the lead never prompts agents, the event manager watches every agent, and the lead pauses with no AI running.

---

# 5A. Model Catalog and Tier Selection

The lead picks a **level** of model for every milestone, not a model name. Preserve 4.7.0:

- **Catalog**: `models.default.toml` shipped with the binary, overrides in `~/.config/herdr-orchestra/models.toml`. Each model has the agent kinds that can run it, a tier (`small`, `medium`, `high`), 1–5 ratings for reasoning, coding, architecture, instruction following, tool use, planning and speed, its context window, whether it has open weights, and the **evidence** for the ratings with a date.
- **Tiers** have a reasoning floor (small 1, medium 3, high 4): coding skill alone never makes a model `high`.

  | Tier | For | Examples |
  |---|---|---|
  | small | renames, docs, config, mechanical edits | `haiku`, `gpt-6-luna` |
  | medium | normal implementation, tests, debugging in one module | `sonnet`, `gpt-6-sol` |
  | high | the lead, contracts, cross-module work, auth / payments / schema, escalations | `opus`, `deepseek-v4-pro`, `glm-5.3` |

- **Policy**: `[policy] deny` keeps ultra-premium models out of every role (default `*fable*`, `*astra*`, `best`). The plan validator refuses a plan that names one, and the launcher refuses to build the command. Opus, Sonnet, Haiku, normal Codex models, Gemini and open-weight models are allowed. Use what the developer's installed agents already reach; open models are optional.
- **Plan fields**: `Workers: claude-1 (claude: sonnet)`; per milestone `Tier:`, `Needs:` (e.g. `coding, tool-use`), `Model why:`.
- **Validation warnings**: no coder reaches a milestone's tier; the reviewer is weaker than the work; the lead is not on a high-tier model; a model is not in the catalog (still works, just has no tier).
- **Tier-aware dispatch**: a milestone goes to a free coder of its tier, or a stronger one if none is free; never a weaker one while a suitable coder exists.
- **One escalation**: a milestone out of fix or review rounds gets one fresh round on a stronger coder, with the work so far, before it fails.
- **Planned against actual**: every attempt records its model; the report has a table of planned tier vs the model that really did the work, and per-model first-pass rate and escalations, so the developer can re-rate models.
- Commands: `orch models`, `orch models show <id>`, `orch models check <id>`, `orch agents launch <kind> --model <id>`.

---

# 6. Developer Experience and Application Design

Build two primary interfaces sharing the same Rust core.

## A. CLI

Provide a developer-friendly command-line interface. It is also the interface **the lead agent uses** (the lead runs `orch` commands in its pane), so its output must stay short, stable and machine-readable.

The CLI must support:

- Installation verification.
- Runtime verification.
- Agent discovery, skills sync and team building.
- Model catalog.
- Project management.
- Planning (validation / sync).
- Execution (dispatch, events, answer, fix, approve).
- Status.
- Logs.
- Reports.
- Configuration.
- Recovery.
- Cleanup and finish.

## B. GUI app (on a local REST API)

Build a desktop GUI app. It is the main experience for developers **who are not inside the lead's pane**. The lead's pane in Herdr stays the conversation with the lead; the GUI shows the run and offers the developer's actions.

How it fits together:

```
   GUI app (Tauri window, web UI)              orch CLI (developer, scripts, and the lead agent)
              │  REST + live events                         │
              ▼                                              ▼
   ┌─────────────────── orch core (one Rust library) ───────────────────┐
   │ plan · scheduler · event manager · agents · models · git · checks │
   └─────────────────────────────────┬──────────────────────────────────┘
                                     ▼
                           Herdr (panes, agents)
```

- **One core, three front doors.** The engine is a Rust library. The CLI, the REST API and the GUI all call it; none of them has its own logic. Under the hood the run works exactly like herdr-orchestra today: the lead plans and pauses, the event manager drives, Herdr runs the agents.
- **Local REST API.** `orch serve` (started by the GUI, or by hand) listens on `127.0.0.1` only, with a per-install token. It offers the same actions as the CLI (status, plan, dispatch, answer, fix, approve, pause, resume, stop, report, agents, models, settings) and a live event stream (Server-Sent Events or WebSocket) so screens update without refreshing. The API is documented (OpenAPI) so other tools can use it later.
- **Not a second driver.** The GUI and the API read state and send *requests* the event manager carries out (§3).
- **Example:** a coder asks "should deleted items be hidden or removed?". The GUI shows a badge on M2, the developer types the answer in the GUI, and the GUI calls the same code as `orch answer M2 "hide them"`. The same coder carries on.

It must be visually clean, intuitive, keyboard-friendly, and responsive.

Build the GUI only after the core orchestration engine is reliable from the CLI (Phase 9 comes after Phases 2–8).

### Main navigation

Implement these screens:

1. Overview.
2. Projects.
3. Agents.
4. Workflows.
5. Execution Monitor.
6. Reports.
7. Settings.

### Overview

Display:

- Connected Herdr runtime.
- Current project.
- Active orchestration and its lead mode.
- Lead state: paused / woken / unread events.
- Total workers.
- Running tasks.
- Completed tasks.
- Blocked, held and failed tasks.
- Overall progress.
- Current feature branch.
- Recent execution events.
- System health.

### Projects

Developers must be able to:

- Add a project.
- Select a project directory.
- Detect its Git repository.
- View the current branch.
- Configure project-specific settings.
- View execution history.
- Start a new orchestration.
- Resume previous orchestration.

Allow multiple projects.

### Agents

Display:

- Available agents.
- Installed agents.
- Authentication status.
- Skills installed or missing.
- Runtime status (and `detect:off`).
- Assigned roles.
- Model and tier of each agent.
- Active sessions.
- Worker availability and benches (cooldowns).

Allow developers to:

- Scan for agents.
- Connect existing agents.
- Add custom agents.
- Remove agents from the registry.
- Configure roles and models.
- Build a team from one tool.
- Configure execution limits.
- Test agent connectivity (`doctor`).
- Inspect agent output.
- Enable or disable agents for a project.

### Workflows

Display:

- Lead planning and the Feature breakdown.
- Milestones, each with its `Why separate:` reason.
- Dependencies, each with its reason.
- Tier, needs and model reason per milestone.
- Acceptance criteria.
- Assigned workers.
- Task states.
- Execution order.
- Parallel task groups.

Allow developers to inspect and approve plans.

### Execution Monitor

Display:

- Active orchestration.
- Agent sessions.
- Task progress.
- Terminal output through Herdr integration.
- Agent state.
- The lead's event queue (open and acknowledged events).
- Verification results.
- Review results.
- Errors.
- Retries and escalations.
- Blockers, questions and held milestones.
- Event history.

Provide actions to:

- Pause.
- Resume.
- Stop.
- Retry (= re-dispatch).
- Approve a held milestone.
- Answer a question (`orch answer`).
- Request a fix after the summary (`orch fix`).
- Inspect logs.
- Open the associated Herdr workspace.

IMPORTANT:

Do not recreate terminal panes inside Orchestra.

Show available terminal output and session information through Herdr's supported interfaces. Provide an option to open the actual Herdr workspace.

### Reports

Provide:

- Final execution summary.
- Completed milestones.
- Failed milestones.
- Review findings (and reviewer suggestions not applied).
- Verification results.
- Git changes, and files changed outside each milestone's Likely files.
- Planned tier vs actual model, escalations.
- Execution duration.
- Agent usage information and model usage (tokens/cost only when agents report them).
- Retry history, first-pass rate and wasted work.

### Settings

Organize settings into:

- General.
- Herdr runtime.
- Agent providers.
- Agent roles.
- Models and policy.
- Lead mode.
- Scheduling.
- Parallel execution.
- Git.
- Verification.
- Review.
- Risk hold and scope.
- Recovery.
- Notifications.
- Storage.
- Advanced configuration.

---

# 7. Minimum Setup and Advanced Setup

The onboarding experience is extremely important.

A new developer should not need to understand internal orchestration architecture to use the application.

Provide two configuration levels.

## Minimum Setup

The minimum setup should require only:

1. Install Herdr Orchestra.
2. Verify the existing Herdr runtime.
3. Select a project directory.
4. Automatically discover installed AI coding agents.
5. Verify their launch and authentication status.
6. Install the skills into each agent (automatic).
7. Select available agents (or let the lead build the team from one installed tool).
8. Assign default roles (the lead suggests them in the plan; the developer can change them).
9. Start using Orchestra: open one lead pane and describe the feature.

Use sensible defaults for:

- Lead mode (`events`).
- Model tiers and deny policy.
- Scheduler.
- Worker limits.
- Git isolation.
- Verification.
- Retry policies.
- Review.
- Risk hold.
- Recovery.
- State storage.

The application should be usable without manually editing TOML files.

Do not force developers to install every supported agent.

If only Codex is installed, the application works with Codex: the lead builds a Codex team (`orch team codex`) and opens the panes (`orch team up`).

If Claude and Codex are installed, allow both to participate.

If no agent is detected, clearly explain the installation and setup requirements.

## Advanced Setup

Provide optional advanced configuration for:

- Custom agent executables.
- Custom launch arguments.
- Custom environment variables.
- Multiple agent instances.
- Agent-specific model settings.
- Custom model ratings, tiers and deny list.
- Custom roles.
- Custom agent prompts.
- Custom planning instructions.
- Lead mode.
- Maximum concurrent workers.
- Per-provider concurrency limits.
- Custom task templates.
- Custom verification commands.
- Custom acceptance criteria.
- Git branch strategy (`Base:`, `Commits: per-milestone | single`).
- Worktree strategy (`worktree` / `shared`).
- Retry policies.
- Timeouts.
- Cooldowns.
- Run budget (`Budget: 3h`).
- Human approval requirements (`Risk hold: on | off`, `Scope: warn | strict`).
- Review policies.
- Event processing settings (lead wake intervals, auto-dispatch delay).
- Logging levels.
- Notification preferences.

Keep advanced configuration optional.

---

# 8. Lead Agent and Planning System

The Lead is an AI agent in a Herdr pane, chosen by the developer by starting the plan playbook in that pane (`/orch-plan`, `$orch-plan`, or "Run `orch lead plan` and follow it"). It names its pane `lead` and writes `Lead: lead (<kind>)` into the plan. It should run on a high-tier model.

The Lead decides **what** and **why**. It never writes application code, never tells coders how to implement or fix, and never prompts or polls agents.

### Planning

The Lead must:

1. Understand the feature request (Feature · Why · Must · Must not · Already decided · Done when · Mode).
2. Research the codebase (including monorepo apps and packages).
3. Identify existing architecture.
4. Identify relevant files and modules.
5. Understand existing conventions.
6. Identify shared interfaces.
7. Identify possible conflicts and shared hotspot files.
8. See which agents and models the team has (and build the team from one tool if it is alone).
9. Write a **Feature breakdown**: the real pieces of work and how they connect.
10. Turn it into milestones, starting from one and splitting only for a reason.
11. Define task dependencies, each with a reason.
12. Give each milestone a tier with a reason.
13. Define acceptance criteria.
14. Define verification requirements (the check command, which must pass on the baseline).
15. Produce a structured plan (`.orchestrate/plan.md`) and wait for the developer's **go**.

### Milestone requirements

Every milestone should contain:

- Unique ID (`M1`, `M2`, …) and an on/off box (`## [x] M1: …`; untick to skip).
- Title.
- Goal and Why.
- `Why separate:` (required when the plan has 2+ milestones).
- Assigned role.
- `Tier:`, `Needs:`, `Model why:`.
- Expected output.
- Dependencies, each with what it needs (`Depends on: M1 (the categories API)`).
- Acceptance criteria (numbered `AC1…` in `task.json`).
- Interfaces (for milestones that run in parallel and must fit together).
- Relevant files (`Likely files:`).
- Constraints and Out of scope.
- Context (facts only, never a how-to).
- Open questions / Notes from user.
- Verification commands.
- Review yes/no.
- `Prefer:` a worker kind or name when a tool suits the work better (a preference, not a lock; the tier comes first).
- Optional `Parallel: allow`.
- Completion requirements.

### Planning rules

- **No fixed milestone counts.** The feature decides. A one-field change that touches API and UI is one milestone, even with five coders. More coders never means more milestones.
- Split a piece off only when that buys something: another coder can build it in parallel, other work has to build on it first, or the whole is too big for one sitting. Write that reason in `Why separate:`.
- Do not create milestones that are too small to be independently meaningful.
- Do not create artificial dependencies. Test each pair: "Could B be built and pass its checks if A never existed?" No → dependency. Yes → parallel.
- Define interfaces before assigning independent frontend and backend tasks. Contract first: parallel milestones that must fit together depend on a small milestone that defines the interface.
- Scaffold first: edits to shared hotspot files (routes, nav, `package.json`/lockfile, shared types) go in one small first milestone; only it adds packages.
- Integration last.
- Identify shared-file conflicts before parallel execution; milestones sharing `Likely files` run one after the other unless `Parallel: allow`.
- A feature made of several loosely connected parts is better as separate runs.
- An optional hard warning above N milestones stays available (`ORCH_MAX_TASKS`, unset by default).

### Lead lifecycle (decision D1)

CRITICAL:

The Lead must not be required to remain active (running, polling, or holding a session) throughout the execution.

A run works like a call stack:

1. The developer says **go**. The lead validates the plan, starts the run, dispatches the milestones whose dependencies are met (`orch dispatch M1 M2`) and **pauses**. No AI runs while it is paused.
2. The event manager does everything else: sends work, watches agents, runs checks, sends fixes and reviews, merges, recovers.
3. The event manager **wakes the lead** with one short prompt ("run `orch events`") only when a decision is needed: a milestone landed and unlocked new work (`READY`), a coder asked a question, a milestone failed or was held, the event manager stopped, or everything is done.
4. The lead handles the batch (dispatch, answer, explain a failure, ask the developer) and pauses again.
5. At `ALL_DONE` the lead adds design notes and a manual verification checklist to the summary.

Rules:

- The Lead is never responsible for waking up coders, watching agents, retrying, or coordinating routine task completions. Routine recovery belongs to the event manager.
- **A run never stalls on the lead.** Ready milestones the lead has not dispatched start by themselves after `ORCH_LEAD_WAIT_MIN` (15). A lead pane that is gone is reported once.
- Everything the lead needs is on disk (`orch events`, `orch status`, plan, summary). A fresh lead session can take over at any time.
- `Lead mode: off` runs with no lead at all: the event manager dispatches by itself and stops when it needs a human.
- Decided in D1: the lead dispatches ready work, with the 15-minute fallback.

---

# 9. Independent Event Manager (Event Coordinator)

Implement a dedicated event manager. It is the reworked `orch autopilot`, and the **only driver** of a run.

This is an orchestration subsystem, not another AI agent.

Its responsibility is to process events and coordinate execution without requiring an LLM.

Important events (names are proposals; map each to today's events):

- ProjectCreated.
- PlanCreated.
- PlanSynced (plan.md read into state; warnings and errors).
- PlanApproved (developer said go).
- RunStarted (feature branch created, agents found).
- TaskCreated.
- TaskReady.
- TaskDispatched (by the lead, by `orch fix`, or `AUTO-DISPATCHED` after the wait).
- TaskAssigned (coder picked, with its model).
- PromptRecorded / PromptConfirmed / PromptUnconfirmed.
- AgentStarted.
- AgentWorking.
- AgentBlocked (permission prompt, first-run question).
- AgentGone / AgentBenched (rate limit, unreachable).
- AgentCompleted (done file / result file).
- QuestionAsked (`QUESTION:` from a coder → task `needs-answer` path).
- AnswerProvided (`orch answer`, from the lead **or the developer**; see §15A).
- TaskCompleted.
- VerificationStarted.
- VerificationFailed.
- VerificationPassed.
- ReviewStarted.
- ReviewCompleted.
- FixRequested (check failure, review blocker, conflict, integration-fix, unmet criterion).
- FollowUpRequested (`orch fix` after the summary).
- Escalated (moved to a stronger model).
- TaskHeld / TaskApproved (risk hold).
- MergeStarted.
- MergeCompleted.
- TaskFailed.
- WorkerUnavailable.
- HumanInputRequired.
- LeadWoken / EventsAcknowledged.
- RunStopped (red baseline, budget used up, branch failing its check).
- OrchestrationCompleted (`ALL_DONE`).

### Event behavior

When an event occurs:

1. Validate the event.
2. Update the relevant task or worker state through the transition function.
3. Persist the state transition **and** the lead's queue entry in the same transaction.
4. Determine affected dependencies.
5. Notify the scheduler.
6. Dispatch eligible work if capacity is available (and the lead mode allows it).
7. Wake the lead if the event is one it must decide on.

### The lead's event queue (preserve 4.7.0)

- Every lead-relevant event is stored in the same write as the change that caused it, so nothing is lost if the process dies.
- The lead is woken **only when it is idle**, and **once per batch** (several events, one prompt).
- An unread batch is sent again after `ORCH_LEAD_REWAKE_S` (300 s), up to `ORCH_LEAD_MAX_WAKES` (3) times; then the developer is told (desktop notification).
- Reading `orch events` is the acknowledgement; `--peek` looks without acknowledging.
- **Wake-up delivery (new in Rust, §15A item 5).** A wake-up is not done until the lead acknowledges it:
  1. **Confirm delivery.** After waking the lead, watch for the acknowledgement (`orch events` read). If none arrives within a short confirm window (a tunable, much shorter than `ORCH_LEAD_REWAKE_S`) while the lead pane is `idle`, wake it again, up to a limit.
  2. **Don't type into a busy or focused pane.** Hold the wake-up while the lead pane is `working`, or focused with input in progress, and send it once the pane is idle.
  3. **Escalate when the wake-up is never acknowledged.** Send a desktop notification ("the lead has N unread events since HH:MM"), the "lead that never reads its events" case in §15 and the notifications list.
  4. **Log it accurately.** Log `wake sent`, then `wake acknowledged` or `wake unacknowledged after Ns`, so the log tells the two apart.
- Blocking events (question, failed, held) stay open until their milestone moves on.
- While the event manager waits for the lead or for the developer it stays up, and that waiting time does not count against `Budget:`. It gives up after `ORCH_LEAD_IDLE_MAX_MIN` (720).

Implement an in-process asynchronous event architecture initially.

Persist important events so execution can recover after a crash.

Use idempotency protections to prevent duplicate event processing.

Use polling only as a recovery and reconciliation mechanism, not as the primary normal execution mechanism, **if Herdr offers a documented way to receive status changes**. Today the Python edition polls herdr status and done files; until Herdr has an event stream, bounded polling of those is the normal path, with long waits and a short return when no agent is working.

Do not introduce Kafka, Redis, RabbitMQ, or distributed infrastructure without a demonstrated need.

---

# 10. Deterministic Scheduler

The scheduler must be independent of AI agents.

It should make decisions using task state, dependencies, worker availability, model tiers, priorities, and configured policies.

Responsibilities:

- Identify ready tasks.
- Resolve dependencies.
- Find suitable workers: a free coder of the milestone's tier, or a stronger one; never a weaker one while a suitable coder exists.
- Prefer the coder who last worked on the milestone (for fix jobs, conflicts and `orch fix`).
- Assign tasks.
- Respect worker capacity and benches.
- Support parallel execution.
- Prevent duplicate assignments.
- Handle blocked tasks.
- Dispatch fix and conflict jobs before new milestones.
- Serialize milestones that share `Likely files` (unless `Parallel: allow`).
- Resume interrupted work.
- One busy or unsuitable coder must not block the dispatch of other ready milestones in the same pass.
- Avoid assigning incompatible tasks to the same worker.

Implement atomic task claiming.

A task must never be assigned to multiple workers simultaneously. In particular, a prompt whose delivery is unconfirmed after a crash is confirmed with Herdr or re-sent to the **same** agent, never handed to a second coder.

### Parallel execution

Independent milestones should execute concurrently (worktree mode).

Dependent milestones must wait until their prerequisites satisfy the required completion conditions.

Support configurable concurrency limits.

Do not create artificial dependencies just to simplify scheduling.

### Review pipelining (preserve 4.2.0)

When a milestone passes its checks, its work is saved to its hidden ref and the coder is freed straight away (`handoff`), so it takes the next ready milestone while the reviewer works. Review blockers come back as a fix job for the next free coder, preferring the one who built it.

---

# 11. Task State Machine

Implement a strongly typed task state machine that matches today's transition table (`TRANSITIONS` in `bin/orch`).

Primary states:

- Pending.
- Coding.
- Verifying.
- AwaitingReview.
- Reviewing.
- Reviewed.
- Committed.
- Merged (terminal).

Additional states:

- NeedsAnswer.
- NeedsFix.
- Held (waiting for `orch approve`).
- Paused.
- Failed.
- Conflict.
- Skipped.

`Ready` and `Assigned` from the old plan are **derived views** (pending + dependencies met; coding + a worker), not stored states, so the table stays the same as the Python one and in-flight runs can be imported.

Today's allowed transitions (keep them unless the parity document says otherwise):

```
pending        -> coding, needs-answer, skipped, paused, failed
needs-answer   -> pending, skipped
skipped        -> pending
coding         -> verifying, needs-fix, conflict, pending, paused, failed
verifying      -> coding, awaiting-review, reviewing, committed, merged, needs-fix, held, pending, paused, failed
awaiting-review-> reviewing, pending, paused, failed
reviewing      -> reviewed, awaiting-review, pending, paused, failed
reviewed       -> needs-fix, committed, merged, held, pending, paused, failed
needs-fix      -> coding, pending, paused, failed
committed      -> merged, conflict, needs-fix, held, pending, paused, failed
held           -> committed, pending, paused, failed
conflict       -> coding, pending, paused, failed
paused         -> coding, skipped, pending, failed
failed         -> skipped, pending, paused
merged         -> (none)
```

Every state transition must be validated.

Manual changes from the CLI are limited to `pending`, `paused`, `failed` and `skipped`; anything else needs `--force` and is logged.

Store:

- Previous state.
- New state.
- Timestamp.
- Triggering event.
- Worker and model.
- Reason.
- Relevant execution details.
- Round counters (fix, review, conflict, lost attempts, escalation used).

Avoid direct, uncontrolled modifications to task state.

Use well-defined transition functions.

---

# 12. Coder Agent Execution

Coders perform the actual implementation. They own the **how**: design, code, tests, fixing failed checks and review findings, resolving conflicts.

Each coder receives an independent task contract: `task.json`, generated from `plan.md` (see `docs/contracts.md`).

The contract must include:

- Task description, goal and why.
- Objective.
- Relevant codebase context (facts).
- Numbered acceptance criteria.
- Dependencies, and the commits of merged dependencies.
- Constraints and out of scope.
- Shared interface definitions.
- Allowed files (`Likely files`).
- Tier and needs.
- Expected output.
- Verification commands.
- Reporting requirements.
- Notes from user (answers to earlier questions).

Coders must:

1. Inspect relevant existing code.
2. Implement the assigned task.
3. Write or update tests.
4. Run relevant checks.
5. Fix implementation failures.
6. Report completion in `result-N.json`: status, commit message (what changed, e.g. `feat(api): add categories endpoint`), files changed, the check they ran, each criterion met or not, optional usage (tokens/cost).
7. Provide a summary of changed files (in the result file).
8. Report questions (`QUESTION:`) and remaining blockers (`BLOCKER:`) instead of guessing.

The line formats `COMMIT:` / `QUESTION:` / `BLOCKER:` keep working when the JSON file is missing. An invalid result file gets one request to rewrite it.

Coders must not independently change unrelated architecture or scope.

Coders must not modify orchestration state directly.

They should report execution outcomes through the supported task completion interface (the done file and the result file).

**Scope check**: the coder's real diff is compared with `Likely files`. `Scope: warn` (default) lists strays in the summary; `Scope: strict` sends the milestone back.

---

# 13. Git and Worktree Management

Implement reliable Git integration that keeps the 4.5 flow arun asked for: **orchestration bookkeeping never leaks into the repo's branches or history.**

Support:

- **One feature branch per run** (`feature/<slug>`, from the plan's `Branch:`), created when the run starts. Planning touches no git at all. It is the only branch a run makes.
- Optional `Base: <branch>`.
- **Detached** git worktrees for coders (`../<repo>-wt/<name>`), not per-worker branches.
- Work in progress (under review, parked, waiting for a fix or conflict) kept in **hidden refs** `refs/orch/<branch>/Mn`: not listed by `git branch`, never pushed.
- One commit per landed milestone, with a real message (the coder's `COMMIT:` line) and a `Milestone: Mn` trailer. No `orch(Mn)` or `wip(Mn)` commits.
- `Commits: single` squashes the run into one commit at finish.
- Squash merge of each milestone onto the feature branch.
- **Check before commit**: squash, run the check on the merged result, commit only if it passes. Passing alone but failing once merged commits nothing and goes back as an `integration-fix` round.
- **Merge journal**: a crash mid-merge is finished (if the commit landed) or undone and redone from the saved work on the next start.
- Merge conflict detection and conflict resolution assignments: the same coder resolves it (up to 2 rounds); an already-approved milestone is reviewed again with the resolution patch.
- Setup command re-run in a worktree when its lockfiles or manifests changed since its last setup (`Setup inputs:` to override), and in the main folder before the merge check.
- Integration testing.
- `orch finish [--no-push]`: final check, remove worktrees and hidden refs, push the feature branch and open a PR into the base (with `gh`, or print the compare link). The event manager never pushes by itself.
- Safe cleanup.
- Pick up work left by older versions (e.g. old `orch/<slug>-Mn` branches) into hidden refs.

### Worktree mode

Default mode for independent parallel tasks.

Each worker receives an isolated working directory.

### Shared mode

Optional mode for tightly coupled or sequential tasks. One milestone at a time in the developer's folder; unfinished work is parked into the milestone's hidden ref so the tree is clean between milestones.

Do not allow concurrent workers to overwrite one another's changes.

Detect shared-file hotspots before execution.

Use Git CLI initially where it offers better compatibility with existing Git workflows.

Do not automatically merge unverified changes.

The final feature branch should be reviewed and merged into the user's main branch only with appropriate human approval (the PR).

---

# 14. Verification, Reviewer and Fixes

Implement a separate reviewer workflow.

The reviewer must be independent of the coder who produced the implementation. It may be another pane of the same tool as the coders. It should not be on a weaker model than the work it reviews.

Reviewer responsibilities:

- Inspect code changes (diff against the merge-base).
- Evaluate acceptance criteria (a verdict per criterion in `review-N.json`).
- Detect functional issues.
- Detect architectural inconsistencies.
- Detect missing tests.
- Identify security concerns.
- Report blockers as problems only: `BLOCKER: file:line — what — why`.
- Provide suggestions (listed in the summary if not applied).

The reviewer must not directly modify implementation files or prescribe fixes.

### Verification

Support configurable:

- Build checks.
- Unit tests.
- Integration tests.
- Lint checks.
- Type checks.
- Custom commands.
- Acceptance criteria verification.

The check command must pass on the **baseline** before any work starts; a red baseline stops the run with the failing lines (the lead proposes a narrower check the developer agrees to).

Verification must happen before final integration (check before commit, §13).

### Review outcomes

Support:

- Approved.
- Approved with warnings.
- Changes requested.
- Blocked.

Use bounded fix cycles: 3 fix rounds for failed checks, 2 review rounds, 2 conflict rounds, then **one escalation** to a stronger model, then failed (and the lead is woken).

Do not allow infinite review and correction loops.

### Risk hold

A milestone that deletes files, touches migrations, CI configuration or secrets files, changes dependency files it didn't list, or exceeds `ORCH_RISK_MAX_LINES` (1500) is **held** before it lands until the developer runs `orch approve Mn`. Other milestones keep going. `Risk hold: off` disables it. The lead never approves a hold.

### Fixes after the summary (preserve 4.8.0)

When the developer asks for a change after the summary, there is **no new planning round**:

- `orch fix Mn "<change>" [--to <worker>]` adds one small follow-up milestone to `plan.md` (depends on Mn, same Likely files, tier and review setting, a `Why separate:` line), dispatches it straight to the coder who built Mn, and starts the event manager.
- The check and the review apply as for any milestone. It works after `ALL_DONE` and during a run. The developer or the lead may run it.
- When the summary is written again, the earlier one is kept (`summary.prev.md`), so the lead's notes are not lost.

Example: summary is done, developer says "the login button should be blue" → lead runs `orch fix M1 "the login button should be blue"` → the coder who built M1 gets it → check → review → new summary.

---

# 15. Failure Recovery

The orchestration engine must be fault-tolerant.

Support (with today's behaviour as the baseline):

| Situation | Behaviour to keep |
|---|---|
| Agent launch failure | reported with the start command (`orch agents launch <kind> --model <id>`) |
| Agent timeout | over `ORCH_CODER_BUDGET_MIN` (45) / `ORCH_REVIEW_BUDGET_MIN` (15): paused, lead woken |
| Agent stops early | nudged once, then parked |
| Agent disappearance | coder: task moves on with the work so far; reviewer: only the review waits; benched `ORCH_GONE_COOLDOWN_MIN` (10), ends as soon as it is back |
| Agent cooldown | `orch cooldown <agent> <min> [reason]` benches any agent (also the reviewer); `gone` ends the bench as soon as herdr sees it again |
| Usage limit / rate limit | bench `ORCH_COOLDOWN_MIN` (60); other coders continue with the work so far |
| Prompt cannot be delivered | task back in the queue with its job kept; agent retried after 2 min; 3 in a row bench it 30 min |
| Lost attempts keep happening | after `ORCH_MAX_LOST` (4) the task fails with the history instead of looping |
| Verification failure | last 60 lines back to the coder, up to 3 rounds |
| Review failure | problem-only fix job, up to 2 rounds |
| Out of rounds | one escalation to a stronger model, then failed |
| Merge conflict | same coder resolves, up to 2 rounds, re-review if already approved |
| Passes alone, fails merged | nothing committed, `integration-fix` round |
| Interrupted process / crash mid-step | unconfirmed prompt confirmed or re-sent to the same agent; half-done merge finished or undone |
| Interrupted Git operations | merge journal (§13) |
| Lockfiles changed | setup re-run |
| Orchestra error in one task | that task paused with the error; the others keep running |
| Final check fails on the branch | stop; the next run re-checks and carries on once it passes |
| Permission prompt | desktop alert; other tasks keep running (see §15A) |
| Lead wake-up not acknowledged | re-woken after a short confirm window while its pane is idle, never typed into a busy or focused pane; desktop notification when it stays unread (see §9 *Wake-up delivery*, §15A item 5) |
| Run budget used up | stop (`Budget: 3h`) |
| Invalid or duplicated events | rejected by validation and idempotency keys |

### Recovery policies

Implement configurable retry limits.

Track retry counts and reasons.

A failed task should not automatically stop independent tasks.

Blocked tasks should pause only their dependent tasks wherever possible.

Support human clarification (preserve 4.7.0: the answer returns to the **same** coder):

When an agent requests human input:

1. Record the question.
2. Mark the task as needing an answer (paused with the question).
3. Pause dependent tasks.
4. Allow independent tasks to continue.
5. Wake the lead. If the request, the plan or the code already answers it, the lead answers (`orch answer Mn "<answer> (from: <where>)"`). Otherwise the lead asks the developer with 2–3 concrete options and the developer is notified.
6. The answer is written under the milestone's "Notes from user" in `plan.md`.
7. The **same coder** resumes from its saved work.

Implement graceful shutdown and safe resume.

Never claim that an incomplete task is completed.

---

# 15A. Known gaps to fix by design in the Rust edition

Found in the real runs of 2026-11-12 and 2026-11-13 and not yet fixed in Python. The Rust edition should solve them, not copy them:

1. **Permission prompts stall unattended runs.** Claude Code launched with `--permission-mode acceptEdits` stops at every shell command. The event manager only alerts, so the run waits for a human. Rust: detect the prompt through Herdr, surface it in the GUI and notifications with the agent and command, and ship launch profiles per tool that allow the project's check and build commands without prompting (configurable, never a blanket "skip all permissions" by default).
2. **An answer typed by the developer is invisible to the lead.** `orch answer` run from the developer's own terminal leaves no event, so the lead is not told the question was settled. Rust: every answer, from anyone, emits `AnswerProvided` into the lead's queue.
3. Tokens and cost show as unknown unless an agent reports `usage`. Keep it optional; read usage from agents that expose it.
4. The opt-in live test (`tests/live/run.sh`) has rarely been run, and Codex / OpenCode have not yet been used as coders in a real run. Make a recorded live run per supported agent part of the release gate (Phase 11).
5. **A wake-up can be sent but never reach the lead.** In the Phase 4 run (2026-11-13, M4), the log showed `woke the lead (lead): 1 event(s) — merged M4` at 23:34:23 and `ALL_DONE` at 23:34:27. The lead never got that prompt as a turn, though, and stayed idle with 2 unread events until the developer asked "status?" by hand. "Woke the lead" only means the prompt was typed into the pane, not that the lead received it. The likely cause is that it landed while the developer was typing in the lead's pane, but this is not confirmed. The 300 s re-wake (§9) would eventually have caught it, but the run sat silent until then, and the log could not tell "sent" from "received". Rust: implement the wake-up delivery rules in §9 (*The lead's event queue*): confirm delivery, hold the wake-up while the lead pane is busy or being typed in, escalate an unacknowledged wake-up, and log sent, acknowledged and unacknowledged separately. A related case from Phase 2: a renamed-away reviewer made review prompts fail with `agent_not_found`, and they were retried silently every 120 s without telling the lead. A prompt that cannot reach its agent must surface to the lead or the developer, never only retry quietly.

---

# 16. Persistent State and Storage

Use SQLite for durable execution state.

Use typed Rust models and database migrations.

Store:

- Projects.
- Orchestrations.
- Plans.
- Milestones.
- Tasks and their transition history.
- Workers and their models.
- Agent configurations.
- Agent sessions.
- Task assignments and attempts (with the model of each attempt).
- Dependencies.
- Events and the lead's event queue (with acknowledgement).
- Pending prompt deliveries (for crash-safe send).
- Merge journal.
- Setup fingerprints per folder.
- Verification results.
- Review reports.
- Retry history.
- Execution summaries.

Keep human-readable project metadata.

Suggested project structure (keeps today's files where they exist):

```
.orchestrate/
    config.toml        (new: project settings that are not plan lines)
    plan.md            (the human source: written by the lead, edited by the developer)
    state.db           (new: authoritative runtime state)
    events.jsonl       (kept: one line per transition, for humans and tools)
    driver.lock        (kept: the single-driver lease)
    autopilot.log      (kept, or renamed with an alias)
    summary.md, summary.prev.md
    logs/
    reports/
    tasks/             (task.json, result-N.json, review-N.json per milestone)
../<repo>-wt/<name>    (worktrees stay outside the repo, as today)
```

`plan.md` stays the file people edit; `orch sync` (or the plan validator) reads it into the database. The database is the authoritative source for runtime state.

Human-readable files should be available for inspection and appropriate editing.

Implement database transactions for critical state transitions (a state change and its queue entry in one transaction).

Ensure safe recovery after unexpected termination.

Do not use state.json as the sole authoritative state store. On first run in a project with an in-flight Python run, import `state.json` (schema 1) so no work is lost (decision D2).

---

# 17. CLI Commands

Preserve and improve the existing `orch` command experience. **Keep every existing command and its meaning** (decision D2), then add new ones.

### Existing commands to preserve

| Group | Commands |
|---|---|
| Lead | `orch lead plan`, `orch lead run`, `orch lead check`, `orch lead whoami` |
| Plan | `orch sync` (validate plan.md, print WARN/ERROR) |
| Run | `orch start` (create the feature branch, check agents), `orch dispatch Mn... [--to w] [--tier t]`, `orch events [--peek]`, `orch answer Mn "<text>"`, `orch fix Mn "<change>" [--to w]`, `orch escalate`, `orch approve Mn`, `orch autopilot` (the event manager) |
| Team | `orch team <kind> [--coders N] [--models a,b] [--no-reviewer]`, `orch team up [--dry-run]` |
| Agents | `orch agents`, `orch agents launch <kind> [--model id]`, `orch agents sync [kind...]`, `orch agents check`, `orch doctor [kind]` |
| Models | `orch models`, `orch models show <id>`, `orch models check <id>` |
| Inspect | `orch status`, `orch ready`, `orch report [--json]` |
| Manual control | `orch set Mn <state>`, `orch park Mn <state> "why"`, `orch cooldown <agent> <min> [reason]`, `orch nudge`, `orch resend`, `orch requeue-review Mn` |
| Finish | `orch cleanup`, `orch finish [--no-push]`, `orch version` |
| Internal (used by the event manager; may become private) | `pick`, `prepare`, `setup-done`, `send`, `send-job`, `handoff`, `needs-fix`, `review`, `review-ok`, `poll`, `bump`, `commit`, `merge`, `write-task` |

### New commands to add

```
orch init                 first-run onboarding (Herdr check, agents, skills)
orch agents scan          discover installed agents and their auth status
orch agents add <kind>    add a custom agent to the registry (no TOML editing)
orch agents remove <kind>
orch agents test <kind>   launch check + auth check (extends orch doctor)
orch plan                 show the plan (milestones, deps, tiers, state)
orch plan validate        alias of orch sync
orch projects             list projects
orch project add <dir>
orch tasks                list milestones with state, worker, model
orch task inspect Mn      contract, attempts, history, results
orch pause / orch resume / orch stop     run-level controls
orch logs [Mn|agent]
orch serve                local REST API for the GUI (127.0.0.1 only)
orch gui                  open the GUI app
```

Name clashes with the old plan, to settle during implementation:

- The old plan's `orch start` meant "start orchestration". Today `orch start` prepares the run and the lead's `orch dispatch` starts the work. Keep today's meaning.
- The old plan's `orch retry` = today's `orch dispatch Mn` on a failed or paused milestone. Add `retry` only as an alias.
- The old plan's `orch plan validate` = today's `orch sync`. Keep both (`plan validate` as an alias).

Provide clear help messages and actionable error messages.

The CLI must support scripting and automation (stable exit codes as today: e.g. 3 = needs a human, 5 = delivery failed, 6 = integration-fix; `--json` where output is structured).

---

# 18. GUI App

Use Tauri (Rust backend, web front end) for the desktop app, talking to the local REST API (§6B).

The GUI must share the same core orchestration engine as the CLI. It is a reader plus a sender of requests; it never becomes a second driver (§3).

Do not recreate terminal panes inside the GUI. Show agent output read through Herdr as text, and offer an "Open in Herdr" button.

Implement:

### Overview

- Current project.
- Runtime connection.
- Active orchestration and lead mode.
- Lead state and unread events.
- Running workers.
- Completed tasks.
- Blocked, held and failed tasks.
- Progress.
- Recent events.

### Projects

- Project selection.
- Project creation.
- Repository information.
- Branch information.
- Execution history.

### Agents

- Installed agents.
- Missing agents.
- Authentication status.
- Skills status.
- Assigned roles and models.
- Worker availability and benches.
- Agent configuration.
- "Build team from one tool".

### Workflows

- Plan inspection, Feature breakdown.
- Milestone graph.
- Dependencies (with reasons).
- Tiers and `Why separate:`.
- Acceptance criteria.
- Task assignments.

### Execution Monitor

- Current execution.
- Agent status.
- Task status.
- Runtime events and the lead's queue.
- Questions waiting for an answer (answer from here).
- Held milestones (approve from here).
- Verification results.
- Review results.
- Errors.
- Retries and escalations.
- Logs.

### Reports

- Execution summaries.
- Completed work.
- Failed work.
- Planned tier vs actual model.
- Verification results.
- Review findings.
- Git changes.

### Settings

- Runtime.
- Agents.
- Models and policy.
- Lead mode.
- Scheduling.
- Git.
- Verification.
- Review.
- Risk hold and scope.
- Recovery.
- Notifications.
- Storage.

Support keyboard navigation, search, filters, confirmations, and useful status indicators.

Make the GUI work well at different window sizes, in light and dark mode. Use desktop notifications for questions, holds, permission prompts and the end of a run.

---

# 19. Minimum Onboarding Experience

The application must be easy to start using.

A developer should be able to:

1. Install Orchestra (curl installer or release binary).
2. Launch Orchestra (open the GUI app, or `orch init` in a terminal).
3. Verify Herdr.
4. Select a project.
5. Scan for agents; skills are installed into each one automatically.
6. Select available agents (or let the lead build the team from what is installed).
7. Confirm default roles and models.
8. Start a task: open one lead pane (any agent) and describe the feature.
9. Read the plan and say **go**.

The system should automatically configure sensible defaults.

Do not require developers to edit configuration files to complete basic onboarding.

Provide helpful messages when Herdr or an agent is missing.

Do not silently install external software. (Installing Orchestra's own skills into agent folders is part of Orchestra's install, recorded in a manifest so uninstall removes exactly that.)

Provide documented installation instructions.

### Installer behaviour to preserve

- `curl -fsSL …/releases/latest/download/install.sh | bash` installs the latest **release**, not `main`. Each release carries its own `install.sh`.
- Every installed version is kept side by side; switching is instant (`--version vX.Y.Z`, `--list`).
- Version choice: `--version` › `ORCH_VERSION` › `./.orch-version` › the release the script came from.
- `--uninstall` removes exactly what the manifest recorded.
- `orch agents sync` installs skills for agents added later, without re-running the installer.
- Skills and slash commands are thin wrappers; the playbooks live inside `orch` (`orch lead plan|run`) so every agent follows the same versioned instructions.

---

# 20. Advanced Configuration

Advanced users must be able to configure:

- Multiple providers.
- Multiple instances of the same agent.
- Custom agents.
- Custom launch commands and model flags.
- Model selection, ratings, tiers and deny list.
- Lead mode.
- Agent-specific roles.
- Custom agent instructions.
- Worker concurrency.
- Task priorities.
- Dependency policies.
- Verification commands.
- Review policies.
- Risk hold and scope policy.
- Git strategies.
- Retry limits.
- Cooldowns.
- Timeouts.
- Run budget.
- Approval policies.
- Event processing (lead re-wake interval and count, auto-dispatch delay, idle maximum).
- Logging.
- Notifications.
- Runtime connections.

Keep configuration project-specific where appropriate.

Provide global and project-level settings with clear precedence rules. Today's sources, which must keep working: plan header lines (full list in Appendix A), the `ORCH_*` environment tunables, `~/.config/herdr-orchestra/agents.toml` and `models.toml`. Proposed precedence: CLI flag › environment › plan header › project `config.toml` › global config › shipped defaults.

Validate configuration before execution.

---

# 21. Rust Technology Stack

Use stable Rust.

Recommended technologies:

- Tokio: asynchronous execution.
- Serde (+ `toml`, `serde_json`): serialization; keep reading the existing TOML registry and catalog and the JSON contracts.
- Clap: CLI.
- Tauri: the desktop GUI app.
- A web front end for the GUI (for example React or Svelte with TypeScript and Vite; pick one in Phase 1).
- Axum: the local REST API, with Server-Sent Events or WebSocket for live updates.
- utoipa or equivalent: OpenAPI description of the API.
- SQLx (or rusqlite): SQLite persistence.
- Tracing: structured logging.
- Notify: filesystem notifications for done and result files.
- Thiserror: error definitions.
- Anyhow: application-level error propagation.
- Tokio process APIs: process management where required.
- `fs2`/`fd-lock` or equivalent: the driver lease and state lock.
- `globset`: deny patterns, hotspot and setup-input globs.
- Petgraph or an equivalent graph implementation: task dependencies if useful.

Use Git CLI initially for Git operations where appropriate.

Use typed interfaces and explicit error handling.

Avoid unnecessary dependencies.

Do not use Rust simply to replace every shell command.

Use the right abstraction for each responsibility.

A single static binary removes today's Python 3.11 requirement.

---

# 22. Security

Implement:

- Secure credential handling.
- No plaintext API secrets in logs.
- No unnecessary environment variable exposure.
- Safe command construction.
- Proper process termination.
- Project directory validation.
- Git operation safeguards (never push without `orch finish`, never force-push).
- Risk hold for deletions, migrations, CI config, secrets files and dependency changes.
- Model deny policy enforced at plan validation and at launch.
- Config validation.
- Permission checks.
- Audit logs (`events.jsonl`, forced transitions logged).

Treat agent output and repository content as untrusted input.

Never execute arbitrary instructions from agent output as orchestration commands.

Only execute configured and validated commands.

Require appropriate confirmation for destructive operations.

---

# 23. Testing Strategy

Testing is mandatory.

Port the Python suite's scenarios first: `tests/run.sh` with the fake herdr (`tests/sim/herdr`) and scripted fake agents (`tests/sim/behave.sh`) covers about 230 cases today. The Rust suite should pass the same scenarios before new features are added.

Implement:

### Unit tests

- Task transitions (the full table).
- Dependency resolution.
- Scheduler decisions, including tier-aware picking and escalation.
- Worker selection.
- Event handling and the lead's queue (batching, re-wake, acknowledgement, auto-dispatch).
- Plan parsing and validation warnings (`Why separate:`, tiers, deny policy, hotspots).
- Team building from one tool.
- Configuration validation.

### Integration tests

- Herdr adapter (against the fake herdr; captured pane fixtures in `tests/fixtures/panes/`).
- Agent discovery and `doctor`.
- Agent launching.
- SQLite persistence and `state.json` import.
- Git worktrees, hidden refs, merge journal, `finish`.
- Verification pipeline and check-before-commit.
- `orch fix` follow-ups.
- REST API endpoints and the live event stream (the GUI's only way in).

### Concurrency tests

- Multiple simultaneous task completions.
- Duplicate event processing.
- Atomic task claiming.
- A second driver refused.
- Concurrent state updates.
- Worker availability changes.

### Recovery tests

- Fault injection at named steps (today `ORCH_FAULT`).
- Process termination.
- Application crash mid-send and mid-merge.
- Interrupted execution.
- Failed Git merge.
- Agent disappearance.
- Retry exhaustion and lost-attempt budget.

### End-to-end tests

Use fake agents and simulated runtime responses.

Do not require real API subscriptions or paid AI usage for normal automated tests.

Create deterministic fixtures.

Keep an **opt-in live smoke test** with real agents (`tests/live/`), run before each release (§15A).

---

# 24. Development Roadmap

Do not implement the entire application in one massive step.

Build in verified phases. Each phase lands as its own PR with a `VERSION` bump and a CHANGELOG entry, and updates the website docs when behaviour changes.

## Phase 1 — Repository Audit and Architecture

- Inspect existing implementation (v4.10.x).
- Document existing behavior.
- Establish feature parity (including everything in section 0's table).
- Apply decisions D1–D3 (section 0).
- Finalize Rust architecture.
- Define contracts.
- Create implementation roadmap.

## Phase 2 — Rust Foundation

- Create Rust workspace.
- Implement CLI skeleton with today's command names.
- Implement configuration (registry, catalog, plan header, tunables).
- Implement SQLite storage and `state.json` import.
- Implement typed task models and the transition table.
- Implement the driver lease.
- Implement logging and errors.

## Phase 3 — Herdr Integration

- Inspect actual Herdr interfaces (CLI and any socket/event API).
- Implement runtime adapter and the fake herdr for tests.
- Connect to existing Herdr.
- Discover workspaces.
- Create and inspect panes (including split from the lead's pane).
- Launch agents.
- Crash-safe prompt delivery.
- Monitor sessions.
- Receive runtime events.

## Phase 4 — Agent and Model Management

- Implement discovery, `doctor`, auth checks.
- Implement registry.
- Implement skill distribution (`agents sync`, `agents check`).
- Implement model catalog, tiers and deny policy.
- Implement role configuration.
- Implement launch configuration.
- Implement `team` and `team up`.
- Test multiple providers.

## Phase 5 — Planning and Task Engine

- Implement lead playbooks (`orch lead plan|run`) and the plan parser.
- Implement plan validation (`orch sync`) with every current warning.
- Implement milestone management and `task.json` generation.
- Implement dependency graph.
- Implement task state machine.

## Phase 6 — Event Manager and Scheduler

- Implement event processing.
- Implement event persistence.
- Implement the lead's event queue and wake-ups, including confirmed wake-up delivery (§9 *Wake-up delivery*, §15A item 5): ack-confirmed re-wake, no typing into a busy or focused lead pane, a desktop notification for unacknowledged wake-ups, and `wake sent` / `wake acknowledged` / `wake unacknowledged` in the log.
- Implement idempotency.
- Implement atomic task claiming.
- Implement tier-aware worker allocation and escalation.
- Implement parallel scheduling and review pipelining.
- Implement `dispatch`, `events`, `answer`, `fix`, `approve`.
- Implement recovery reconciliation.

## Phase 7 — Git Management

- Implement the feature branch.
- Implement detached worktree creation and hidden refs.
- Implement worker isolation and setup freshness.
- Implement check-before-commit and commits.
- Implement merge management and the merge journal.
- Implement conflict handling.
- Implement `finish`.

## Phase 8 — Verification and Review

- Implement automated checks and the baseline check.
- Implement reviewer execution and `review-N.json`.
- Implement fix jobs and `integration-fix`.
- Implement bounded review cycles.
- Implement risk hold and scope check.
- Implement integration validation.

## Phase 9 — REST API and GUI

- Implement the local REST API and live event stream (`orch serve`).
- Implement the Tauri app shell.
- Implement navigation.
- Implement onboarding.
- Implement project management.
- Implement agent and model management.
- Implement workflow visualization.
- Implement execution monitoring (questions, holds, lead queue).
- Implement settings and reports.

## Phase 10 — Recovery and Hardening

- Crash recovery.
- Graceful shutdown.
- Resume.
- Concurrent execution testing.
- Failure simulation.
- Fix the known gaps (§15A).
- Security review.
- Performance testing.

## Phase 11 — Release

- Build release binaries: the `orch` CLI binary and GUI app packages (AppImage and .deb first).
- Installation scripts (keep the curl installer, side-by-side versions, pinning, uninstall manifest).
- Linux support initially.
- Documentation and a website for the Rust edition (the Python repo's `site/` is the starting point).
- Example configurations.
- End-to-end validation, including a recorded live run per supported agent.
- **Automatic releases**: a merge to `main` with a new `VERSION` runs the tests, tags `vX.Y.Z` and publishes the GitHub Release with its own installer (as in PR #16). Nobody pushes tags by hand.
- Release as 5.0.0 (decision D2).

---

# 25. Engineering Rules

Follow these rules throughout implementation.

1. Do not duplicate Herdr functionality.
2. Do not make the Lead responsible for continuous coordination. The lead pauses between decisions; the event manager watches and recovers. A run never stalls on the lead.
3. Do not use LLMs for deterministic scheduling, and never call an LLM API from Orchestra itself.
4. Exactly one driver per run.
5. Do not introduce unnecessary microservices.
6. Do not introduce unnecessary distributed infrastructure.
7. Do not create artificial task dependencies, and do not size plans by a fixed milestone count.
8. Do not permit duplicate task assignment.
9. Do not allow agents to modify authoritative orchestration state directly.
10. Do not commit or merge unverified work (check before commit).
11. Do not create infinite retry loops.
12. Do not lose completed work after a crash.
13. Do not let orchestration bookkeeping leak into the repo (no extra branches, no bookkeeping commits).
14. Do not force developers into advanced configuration.
15. Do not tightly couple to one AI provider; any agent can play any role.
16. Do not run ultra-premium models the policy denies.
17. Do not build a terminal emulator inside Orchestra.
18. Do not implement features without tests.
19. Do not silently remove existing functionality.
20. Do not assume undocumented Herdr APIs.
21. Do not overengineer the architecture.
22. Every feature PR bumps the version and adds a changelog entry.

Prefer simple, testable, explicit designs.

Every module should have a clear responsibility.

---

# 26. Definition of Done

The Rust implementation is considered successful when a developer can:

1. Install Herdr Orchestra.
2. Verify an existing Herdr installation.
3. Select a software project.
4. Automatically discover Claude Code, Codex, or other supported agents, with skills installed into each.
5. Configure their roles and models without manually editing files, or let the lead build the team from a single installed tool.
6. Submit a feature request.
7. Receive a structured Lead-generated plan, sized to the feature, with a tier and reason per milestone.
8. Review and approve the plan.
9. Start orchestration.
10. Let the Lead pause; no AI runs while it waits, and it is woken only for decisions.
11. Have independent workers execute eligible tasks on models of the right tier.
12. Execute independent milestones in parallel.
13. Detect agent completion through reliable runtime and task signals.
14. Automatically unlock dependent milestones.
15. Run verification before anything is committed.
16. Request independent code review.
17. Automatically manage bounded fix cycles and one escalation.
18. Answer a coder's question and have the same coder continue.
19. Approve held risky changes.
20. Recover from agent failures.
21. Resume after application crashes without losing or duplicating work.
22. Manage isolated Git worktrees on one feature branch.
23. Integrate verified changes into the feature branch.
24. View and control the entire execution through the GUI app.
25. Inspect execution history, planned-vs-actual models and final reports.
26. Request a change after the summary and have it go straight to the coder who built that part.
27. Approve the final integration into the main branch through a PR.
28. Pick up a run started with the Python edition (decision D2).

The system must work without the Lead remaining active throughout the run.

The orchestration engine must independently manage the complete lifecycle.

---

# 27. Final Product Definition

Herdr Orchestra is a Rust-based, developer-facing multi-agent software development orchestration platform.

It uses Herdr as its existing terminal and coding-agent runtime while providing an independent orchestration engine for planning, task decomposition, model-tier selection, dependency management, deterministic scheduling, event processing, parallel execution, verification, review, recovery, and Git integration.

An AI lead plans and decides; a plain event manager runs the work and wakes the lead only when a decision is needed.

Its primary interfaces are a GUI desktop app (on a local REST API) and a CLI, sharing one Rust core.

Its primary goal is to make multiple AI coding agents work together as a coordinated software engineering team, with minimal manual intervention and reliable execution.

**Build a system that developers can trust with real software development workflows, not just an impressive multi-agent demonstration.**

---

# Appendix A. Inventory of the Python edition (v4.10.x) to carry over

Use this as the parity checklist. Every item must exist in the Rust edition, or be listed in the parity document as redesigned or removed with a reason.

### Commands (`bin/orch`)

- Lead: `lead plan`, `lead run`, `lead check`, `lead whoami`.
- Plan and run: `sync`, `start`, `dispatch`, `events`, `answer`, `fix`, `escalate`, `approve`, `autopilot`, `ready`, `status`, `report`.
- Team, agents, models: `team <kind>`, `team up`, `agents`, `agents launch`, `agents sync`, `agents check`, `doctor`, `models`, `models show`, `models check`.
- Manual control: `set`, `park`, `cooldown`, `nudge`, `resend`, `requeue-review`.
- Driver steps: `pick`, `prepare`, `setup-done`, `send`, `send-job`, `handoff`, `needs-fix`, `review`, `review-ok`, `poll`, `bump`, `commit`, `merge`, `write-task`.
- Finish: `cleanup`, `finish [--no-push]`, `version`, and `orch` with no arguments (full help).

### Plan header lines (`.orchestrate/plan.md`)

`# Plan: <title>` · `Branch:` · `Base:` · `Check command:` · `Setup command:` · `Setup inputs:` · `Isolation: worktree | shared` · `Max parallel:` · `Lead:` · `Lead mode: events | off` · `Workers:` (with `kind: model` and `@pane:<id>`) · `Reviewer:` · `Commits: per-milestone | single` · `Scope: warn | strict` · `Risk hold: on | off` · `Budget:` · `## Feature breakdown` · `## Global instructions`.

### Milestone lines

`## [x] Mn: <title>` · `Goal:` · `Why:` · `Why separate:` · `Depends on:` (with reasons) · `Acceptance criteria:` · `Constraints` · `Interfaces` · `Context` · `Out of scope` · `Likely files:` · `Tier:` · `Needs:` · `Model why:` · `Prefer:` · `Review: yes | no` · `Parallel: allow` · `Open questions:` · `Notes from user`.

### Tunables (environment)

`ORCH_CODER_BUDGET_MIN` (45) · `ORCH_REVIEW_BUDGET_MIN` (15) · `ORCH_CHECK_TIMEOUT_MIN` (30) · `ORCH_COOLDOWN_MIN` (60) · `ORCH_GONE_COOLDOWN_MIN` (10) · `ORCH_IDLE_GRACE_S` (120) · `ORCH_POLL_STEP_S` (10) · `ORCH_HERDR_TIMEOUT_S` (30) · `ORCH_SEND_RETRY_S` (120) · `ORCH_CONFIRM_S` (30) · `ORCH_MAX_LOST` (4) · `ORCH_UNREACHABLE_AFTER` (3) · `ORCH_UNREACHABLE_MIN` (30) · `ORCH_RISK_MAX_LINES` (1500) · `ORCH_MAX_TASKS` (unset) · `ORCH_LEAD_WAIT_MIN` (15) · `ORCH_LEAD_REWAKE_S` (300) · `ORCH_LEAD_MAX_WAKES` (3) · `ORCH_LEAD_IDLE_MAX_MIN` (720) · `ORCH_NO_AUTOSTART` · `ORCH_TEAM_WAIT_S` · `ORCH_AGENTS_FILE` · `ORCH_MODELS_FILE` · `ORCH_REPO` · `ORCH_VERSION` · `ORCH_FAULT` (test hook) · `HERDR` (path of the herdr binary). Fixed limits: 3 fix rounds, 2 review rounds, 2 conflict rounds.

### Files

- In the project: `.orchestrate/plan.md`, `state.json` (schema 1), `state.lock`, `driver.lock`, `events.jsonl`, `autopilot.log`, `summary.md`, `summary.prev.md`, per-milestone `task.json`, `result-N.json`, `review-N.json`, `diff.patch`, `resolution.patch`; `.orch-task/` done files in each worktree; worktrees in `../<repo>-wt/<name>`; hidden refs `refs/orch/<branch>/Mn`; `.orch-version` pin.
- Shipped: `agents.default.toml`, `models.default.toml`, `playbooks/plan.md`, `playbooks/run.md`, `skills/orch-plan`, `skills/orch-run`, `claude-commands/` (`/orch-plan`, `/orch-run`), `opencode-commands/`, `docs/contracts.md`.
- User overrides: `~/.config/herdr-orchestra/agents.toml`, `~/.config/herdr-orchestra/models.toml`.
- Install locations: `~/.local/bin/orch`, `~/.local/share/herdr-orchestra/` (versions side by side), `~/.agents/skills/`, each agent's skill folder (`~/.claude/skills`, `~/.codex/skills`, `~/.hermes/skills`, `~/.gemini/config/skills`), `~/.config/opencode/commands/`, the uninstall manifest.

### Installer and release

`install.sh` flags `--version`, `--list`, `--current`, `--uninstall`, `--no-herdr-skill`; version order `--version` › `ORCH_VERSION` › `.orch-version` › the release; optional install of the official herdr skill via npx; Python 3.11 fallback (not needed in Rust); `scripts/release.sh`; release workflow that tags and publishes on merge (PR #16); a CHANGELOG entry and version bump in every feature PR.

### Notifications

Desktop notifications (`notify-send`) for things that need the developer: questions, holds, permission prompts, a lead that never reads its events, the run stopping.

### Tests and docs

`tests/run.sh` (about 230 scenarios with fake agents), `tests/sim/herdr` (fake herdr), `tests/sim/behave.sh` (scripted agent behaviour), `tests/fixtures/panes/` (pane text, synthetic so far), `tests/live/run.sh` + `results.md` (opt-in real run), CI on every PR. Docs: `README.md`, `CHANGELOG.md`, `docs/contracts.md`, and the website (`site/`, Astro, on Vercel at herdr-orchestra.vercel.app) with pages for installation, quick start, workflow, planning, plan file, lead, models, agents, modes, git, reviews, driving, when it stops, configuration, commands, files and feature requests.

