# Project: Herdr Orchestra — Rust Edition

## 1. Project Vision

Build a complete, production-quality Rust implementation of Herdr Orchestra, based on the existing project:

https://github.com/arunnarangali/herdr-orchestra

The objective is to evolve the existing Python-based orchestration system into a modular, reliable, event-driven Rust application.

This is not simply a Python-to-Rust translation.

Build a complete developer-facing multi-agent software development orchestration platform that coordinates different AI coding agents, manages their execution, tracks dependencies, handles failures, verifies implementations, and integrates completed work.

The system must be simple enough for a developer to start using within minutes, while providing advanced configuration and control for experienced developers.

### Core philosophy

- Herdr provides the terminal and coding-agent runtime.
- Herdr Orchestra provides orchestration and workflow intelligence.
- AI agents perform software development tasks.
- The scheduler manages task allocation and execution.
- The event coordinator handles execution events.
- Git provides isolation and integration.
- Automated checks provide verification.
- Humans provide decisions and approvals when necessary.

The final product must be lightweight, modular, deterministic, extensible, and fault-tolerant.

---

# 2. First Step: Analyze the Existing Repository

Before writing code, thoroughly inspect:

https://github.com/arunnarangali/herdr-orchestra

Understand the complete existing implementation.

Analyze:

- Existing Python architecture.
- CLI commands.
- Lead planning workflow.
- Coder execution workflow.
- Autopilot implementation.
- Agent registry.
- Herdr integration.
- Task state machine.
- Dependency management.
- Git worktree management.
- Verification pipeline.
- Reviewer workflow.
- Retry and recovery logic.
- Agent cooldown management.
- Human clarification handling.
- Configuration files.
- Skills and playbooks.
- Existing tests.
- Installation and uninstallation.
- Documentation.

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
2. Lead planning interface.
3. Task and milestone management.
4. Dependency engine.
5. Deterministic scheduler.
6. Independent event coordinator.
7. Agent registry and management.
8. Herdr runtime integration.
9. Git and worktree management.
10. Verification and review.
11. Failure recovery.
12. Persistent state management.
13. CLI.
14. Interactive TUI.
15. Reporting and observability.

All interfaces must communicate through well-defined contracts.

Avoid unnecessary microservices.

Start with a modular, asynchronous Rust application.

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

### Herdr responsibilities

Herdr owns:

- Workspace management.
- Tab management.
- Pane creation.
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

- Agent assignment.
- Agent roles.
- Task allocation.
- Execution scheduling.
- Dependency management.
- Task completion decisions.
- Verification.
- Review.
- Recovery.
- Git operations.
- Workflow state.
- Execution history.

### Build a dedicated Herdr integration adapter

The adapter should provide typed Rust interfaces for:

- Connecting to Herdr.
- Checking runtime availability.
- Discovering workspaces.
- Creating or selecting workspaces.
- Creating agent execution panes.
- Launching agents.
- Sending prompts and input.
- Reading terminal output.
- Monitoring agent status.
- Receiving runtime events.
- Detecting agent completion.
- Detecting agent failure.
- Detecting blocked agents.
- Closing or cleaning up sessions where appropriate.

Use the actual capabilities exposed by Herdr.

If a capability is not exposed by Herdr, document the limitation and implement a safe alternative without duplicating its entire runtime.

Make the runtime adapter replaceable so alternative execution backends can be supported in the future.

---

# 5. Agent Management System

The developer must be able to connect and manage multiple AI coding agents easily.

Support configurable agents including:

- Claude Code.
- OpenAI Codex.
- OpenCode.
- Amp.
- Hermes.
- Antigravity.
- Other compatible CLI coding agents.

Do not tightly couple the orchestration engine to individual providers.

### Agent discovery

Implement automatic agent discovery.

When the application starts:

1. Scan configured executable locations and PATH.
2. Identify supported agent CLIs.
3. Check executable availability.
4. Detect installed versions where possible.
5. Validate that each agent can launch.
6. Check authentication status using supported non-destructive mechanisms.
7. Report whether the agent is ready for use.

Keep installation and authentication separate.

An installed CLI does not necessarily mean its account is authenticated.

Never request or store provider passwords or private API credentials unnecessarily.

Use existing provider authentication mechanisms and secure credential storage when API credentials are explicitly configured.

### Agent configuration

Each agent should support:

- Agent identifier.
- Provider.
- Display name.
- Executable.
- Launch command.
- Arguments.
- Supported roles.
- Default model, if configurable.
- Authentication status.
- Runtime status.
- Capabilities.
- Maximum concurrent sessions.
- Execution timeout.
- Cooldown.
- Custom environment variables.
- Working directory.
- Custom agent instructions.

### Agent roles

Support configurable roles:

- Lead.
- Coder.
- Reviewer.
- Tester.
- Researcher.
- Specialist.

Allow multiple agents of the same provider.

For example:

- Claude 1: Coder.
- Claude 2: Reviewer.
- Codex 1: Coder.
- OpenCode 1: Tester.

Do not assume one provider can only have one active agent.

---

# 6. Developer Experience and Application Design

Build two primary interfaces sharing the same Rust core.

## A. CLI

Provide a developer-friendly command-line interface.

The CLI must support:

- Installation verification.
- Runtime verification.
- Agent discovery.
- Project management.
- Planning.
- Execution.
- Status.
- Logs.
- Reports.
- Configuration.
- Recovery.
- Cleanup.

## B. Interactive TUI

Build a professional interactive terminal application using Ratatui and Crossterm.

The TUI should be the main interactive experience for developers.

It must be visually clean, intuitive, keyboard-friendly, and responsive.

Do not build an elaborate GUI before the core orchestration engine is reliable.

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
- Active orchestration.
- Total workers.
- Running tasks.
- Completed tasks.
- Blocked tasks.
- Failed tasks.
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
- Runtime status.
- Assigned roles.
- Active sessions.
- Model configuration.
- Worker availability.

Allow developers to:

- Scan for agents.
- Connect existing agents.
- Add custom agents.
- Remove agents from the registry.
- Configure roles.
- Configure execution limits.
- Test agent connectivity.
- Inspect agent output.
- Enable or disable agents for a project.

### Workflows

Display:

- Lead planning.
- Milestones.
- Dependencies.
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
- Verification results.
- Review results.
- Errors.
- Retries.
- Blockers.
- Event history.

Provide actions to:

- Pause.
- Resume.
- Stop.
- Retry.
- Approve.
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
- Review findings.
- Verification results.
- Git changes.
- Execution duration.
- Agent usage information.
- Retry history.

### Settings

Organize settings into:

- General.
- Herdr runtime.
- Agent providers.
- Agent roles.
- Scheduling.
- Parallel execution.
- Git.
- Verification.
- Review.
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
6. Select available agents.
7. Assign default roles.
8. Start using Orchestra.

Use sensible defaults for:

- Scheduler.
- Worker limits.
- Git isolation.
- Verification.
- Retry policies.
- Review.
- Recovery.
- State storage.

The application should be usable without manually editing TOML files.

Do not force developers to install every supported agent.

If only Codex is installed, the application should work with Codex.

If Claude and Codex are installed, allow both to participate.

If no agent is detected, clearly explain the installation and setup requirements.

## Advanced Setup

Provide optional advanced configuration for:

- Custom agent executables.
- Custom launch arguments.
- Custom environment variables.
- Multiple agent instances.
- Agent-specific model settings.
- Custom roles.
- Custom agent prompts.
- Custom planning instructions.
- Maximum concurrent workers.
- Per-provider concurrency limits.
- Custom task templates.
- Custom verification commands.
- Custom acceptance criteria.
- Git branch strategy.
- Worktree strategy.
- Retry policies.
- Timeouts.
- Cooldowns.
- Human approval requirements.
- Review policies.
- Event processing settings.
- Logging levels.
- Notification preferences.

Keep advanced configuration optional.

---

# 8. Lead Agent and Planning System

The Lead is responsible for understanding requirements and producing an execution plan.

The Lead must:

1. Understand the feature request.
2. Research the codebase.
3. Identify existing architecture.
4. Identify relevant files and modules.
5. Understand existing conventions.
6. Identify shared interfaces.
7. Identify possible conflicts.
8. Break work into milestones.
9. Define task dependencies.
10. Define acceptance criteria.
11. Define verification requirements.
12. Produce a structured plan.

### Milestone requirements

Every milestone should contain:

- Unique ID.
- Title.
- Description.
- Assigned role.
- Expected output.
- Dependencies.
- Acceptance criteria.
- Relevant files.
- Constraints.
- Verification commands.
- Completion requirements.

### Planning rules

Avoid excessive task splitting.

Do not create milestones that are too small to be independently meaningful.

Do not create artificial dependencies.

Identify shared-file conflicts before parallel execution.

Define interfaces before assigning independent frontend and backend tasks.

### Lead lifecycle

CRITICAL:

The Lead must not be required to remain active throughout the execution.

After producing and validating the plan, the Lead may finish its planning responsibility.

The orchestration engine must independently manage all subsequent execution.

The Lead should only be invoked again when additional architectural reasoning or a genuine clarification is necessary.

Do not make the Lead responsible for waking up coders or coordinating every task completion.

---

# 9. Independent Event Coordinator

Implement a dedicated event coordinator.

This is an orchestration subsystem, not another AI agent.

Its responsibility is to process events and coordinate execution without requiring an LLM.

Important events:

- ProjectCreated.
- PlanCreated.
- PlanApproved.
- TaskCreated.
- TaskReady.
- TaskAssigned.
- AgentStarted.
- AgentWorking.
- AgentBlocked.
- AgentCompleted.
- TaskCompleted.
- VerificationStarted.
- VerificationFailed.
- VerificationPassed.
- ReviewStarted.
- ReviewCompleted.
- FixRequested.
- MergeStarted.
- MergeCompleted.
- WorkerUnavailable.
- HumanInputRequired.
- OrchestrationCompleted.

### Event behavior

When an event occurs:

1. Validate the event.
2. Update the relevant task or worker state.
3. Persist the state transition.
4. Determine affected dependencies.
5. Notify the scheduler.
6. Dispatch eligible work if capacity is available.

Implement an in-process asynchronous event architecture initially.

Persist important events so execution can recover after a crash.

Use idempotency protections to prevent duplicate event processing.

Use polling only as a recovery and reconciliation mechanism, not as the primary normal execution mechanism.

Do not introduce Kafka, Redis, RabbitMQ, or distributed infrastructure without a demonstrated need.

---

# 10. Deterministic Scheduler

The scheduler must be independent of AI agents.

It should make decisions using task state, dependencies, worker availability, priorities, and configured policies.

Responsibilities:

- Identify ready tasks.
- Resolve dependencies.
- Find suitable workers.
- Assign tasks.
- Respect worker capacity.
- Support parallel execution.
- Prevent duplicate assignments.
- Handle blocked tasks.
- Prioritize fix jobs when appropriate.
- Resume interrupted work.
- Avoid assigning incompatible tasks to the same worker.

Implement atomic task claiming.

A task must never be assigned to multiple workers simultaneously.

### Parallel execution

Independent milestones should execute concurrently.

Dependent milestones must wait until their prerequisites satisfy the required completion conditions.

Support configurable concurrency limits.

Do not create artificial dependencies just to simplify scheduling.

---

# 11. Task State Machine

Implement a strongly typed task state machine.

Primary states:

- Pending.
- Ready.
- Assigned.
- Coding.
- Verifying.
- AwaitingReview.
- Reviewing.
- Reviewed.
- Committed.
- Merged.

Additional states:

- NeedsAnswer.
- Paused.
- Failed.
- Conflict.
- Skipped.

Every state transition must be validated.

Store:

- Previous state.
- New state.
- Timestamp.
- Triggering event.
- Worker.
- Reason.
- Relevant execution details.

Avoid direct, uncontrolled modifications to task state.

Use well-defined transition functions.

---

# 12. Coder Agent Execution

Coders perform the actual implementation.

Each coder receives an independent task contract.

The contract must include:

- Task description.
- Objective.
- Relevant codebase context.
- Acceptance criteria.
- Dependencies.
- Constraints.
- Shared interface definitions.
- Expected output.
- Verification commands.
- Reporting requirements.

Coders must:

1. Inspect relevant existing code.
2. Implement the assigned task.
3. Write or update tests.
4. Run relevant checks.
5. Fix implementation failures.
6. Report completion.
7. Provide a summary of changed files.
8. Report remaining blockers.

Coders must not independently change unrelated architecture.

Coders must not modify orchestration state directly.

They should report execution outcomes through the supported task completion interface.

---

# 13. Git and Worktree Management

Implement reliable Git integration.

Support:

- Feature branches.
- Independent worker branches.
- Git worktrees.
- Task-level commits.
- Squash merges.
- Merge conflict detection.
- Conflict resolution assignments.
- Integration testing.
- Safe cleanup.

### Worktree mode

Default mode for independent parallel tasks.

Each worker receives an isolated working directory.

### Shared mode

Optional mode for tightly coupled or sequential tasks.

Do not allow concurrent workers to overwrite one another's changes.

Detect shared-file hotspots before execution.

Use Git CLI initially where it offers better compatibility with existing Git workflows.

Do not automatically merge unverified changes.

The final feature branch should be reviewed and merged into the user's main branch only with appropriate human approval.

---

# 14. Verification and Reviewer

Implement a separate reviewer workflow.

The reviewer must be independent of the coder who produced the implementation.

Reviewer responsibilities:

- Inspect code changes.
- Evaluate acceptance criteria.
- Detect functional issues.
- Detect architectural inconsistencies.
- Detect missing tests.
- Identify security concerns.
- Report blockers.
- Provide actionable recommendations.

The reviewer must not directly modify implementation files.

### Verification

Support configurable:

- Build checks.
- Unit tests.
- Integration tests.
- Lint checks.
- Type checks.
- Custom commands.
- Acceptance criteria verification.

Verification must happen before final integration.

### Review outcomes

Support:

- Approved.
- Approved with warnings.
- Changes requested.
- Blocked.

Use bounded fix cycles.

Do not allow infinite review and correction loops.

---

# 15. Failure Recovery

The orchestration engine must be fault-tolerant.

Support:

- Agent launch failure.
- Agent timeout.
- Agent disappearance.
- Agent cooldown.
- Usage limit.
- Verification failure.
- Review failure.
- Merge conflict.
- Interrupted process.
- Application crash.
- Interrupted Git operations.
- Invalid or duplicated events.

### Recovery policies

Implement configurable retry limits.

Track retry counts and reasons.

A failed task should not automatically stop independent tasks.

Blocked tasks should pause only their dependent tasks wherever possible.

Support human clarification.

When an agent requests human input:

1. Record the question.
2. Mark the task as needing an answer.
3. Pause dependent tasks.
4. Allow independent tasks to continue.
5. Notify the developer.
6. Resume when the answer is provided.

Implement graceful shutdown and safe resume.

Never claim that an incomplete task is completed.

---

# 16. Persistent State and Storage

Use SQLite for durable execution state.

Use typed Rust models and database migrations.

Store:

- Projects.
- Orchestrations.
- Plans.
- Milestones.
- Tasks.
- Workers.
- Agent configurations.
- Agent sessions.
- Task assignments.
- Dependencies.
- Events.
- Verification results.
- Review reports.
- Retry history.
- Execution summaries.

Keep human-readable project metadata.

Suggested project structure:

.orchestrate/
    config.toml
    plan.md
    state.db
    events.jsonl
    logs/
    reports/
    tasks/
    worktrees/

The database should be the authoritative source for runtime state.

Human-readable files should be available for inspection and appropriate editing.

Implement database transactions for critical state transitions.

Ensure safe recovery after unexpected termination.

Do not use state.json as the sole authoritative state store.

---

# 17. CLI Commands

Preserve and improve the existing `orch` command experience.

Implement commands such as:

orch init
orch doctor
orch agents
orch agents scan
orch agents add
orch agents remove
orch agents test
orch projects
orch project add
orch plan
orch plan validate
orch start
orch status
orch tasks
orch task inspect
orch approve
orch pause
orch resume
orch stop
orch retry
orch logs
orch report
orch cleanup
orch tui

The exact command names can be refined during implementation.

Provide clear help messages and actionable error messages.

The CLI must support scripting and automation.

---

# 18. Interactive TUI

Use Ratatui and Crossterm.

The TUI must share the same core orchestration engine as the CLI.

Implement:

### Overview

- Current project.
- Runtime connection.
- Active orchestration.
- Running workers.
- Completed tasks.
- Blocked tasks.
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
- Assigned roles.
- Worker availability.
- Agent configuration.

### Workflows

- Plan inspection.
- Milestone graph.
- Dependencies.
- Acceptance criteria.
- Task assignments.

### Execution Monitor

- Current execution.
- Agent status.
- Task status.
- Runtime events.
- Verification results.
- Review results.
- Errors.
- Retries.
- Logs.

### Reports

- Execution summaries.
- Completed work.
- Failed work.
- Verification results.
- Review findings.
- Git changes.

### Settings

- Runtime.
- Agents.
- Scheduling.
- Git.
- Verification.
- Review.
- Recovery.
- Notifications.
- Storage.

Support keyboard navigation, search, filters, confirmations, and useful status indicators.

Make the TUI work correctly at different terminal sizes.

---

# 19. Minimum Onboarding Experience

The application must be easy to start using.

A developer should be able to:

1. Launch Orchestra.
2. Verify Herdr.
3. Select a project.
4. Scan for agents.
5. Select available agents.
6. Confirm default roles.
7. Start a task.

The system should automatically configure sensible defaults.

Do not require developers to edit configuration files to complete basic onboarding.

Provide helpful messages when Herdr or an agent is missing.

Do not silently install external software.

Provide documented installation instructions.

---

# 20. Advanced Configuration

Advanced users must be able to configure:

- Multiple providers.
- Multiple instances of the same agent.
- Custom agents.
- Custom launch commands.
- Model selection.
- Agent-specific roles.
- Custom agent instructions.
- Worker concurrency.
- Task priorities.
- Dependency policies.
- Verification commands.
- Review policies.
- Git strategies.
- Retry limits.
- Cooldowns.
- Timeouts.
- Approval policies.
- Event processing.
- Logging.
- Notifications.
- Runtime connections.

Keep configuration project-specific where appropriate.

Provide global and project-level settings with clear precedence rules.

Validate configuration before execution.

---

# 21. Rust Technology Stack

Use stable Rust.

Recommended technologies:

- Tokio: asynchronous execution.
- Serde: serialization.
- Clap: CLI.
- Ratatui: terminal UI.
- Crossterm: terminal handling.
- SQLx: SQLite persistence.
- Tracing: structured logging.
- Notify: filesystem notifications where needed.
- Thiserror: error definitions.
- Anyhow: application-level error propagation.
- Tokio process APIs: process management where required.
- Petgraph or an equivalent graph implementation: task dependencies if useful.

Use Git CLI initially for Git operations where appropriate.

Use typed interfaces and explicit error handling.

Avoid unnecessary dependencies.

Do not use Rust simply to replace every shell command.

Use the right abstraction for each responsibility.

---

# 22. Security

Implement:

- Secure credential handling.
- No plaintext API secrets in logs.
- No unnecessary environment variable exposure.
- Safe command construction.
- Proper process termination.
- Project directory validation.
- Git operation safeguards.
- Config validation.
- Permission checks.
- Audit logs.

Treat agent output and repository content as untrusted input.

Never execute arbitrary instructions from agent output as orchestration commands.

Only execute configured and validated commands.

Require appropriate confirmation for destructive operations.

---

# 23. Testing Strategy

Testing is mandatory.

Implement:

### Unit tests

- Task transitions.
- Dependency resolution.
- Scheduler decisions.
- Worker selection.
- Event handling.
- Configuration validation.

### Integration tests

- Herdr adapter.
- Agent discovery.
- Agent launching.
- SQLite persistence.
- Git worktrees.
- Verification pipeline.

### Concurrency tests

- Multiple simultaneous task completions.
- Duplicate event processing.
- Atomic task claiming.
- Concurrent state updates.
- Worker availability changes.

### Recovery tests

- Process termination.
- Application crash.
- Interrupted execution.
- Failed Git merge.
- Agent disappearance.
- Retry exhaustion.

### End-to-end tests

Use fake agents and simulated runtime responses.

Do not require real API subscriptions or paid AI usage for normal automated tests.

Create deterministic fixtures.

---

# 24. Development Roadmap

Do not implement the entire application in one massive step.

Build in verified phases.

## Phase 1 — Repository Audit and Architecture

- Inspect existing implementation.
- Document existing behavior.
- Establish feature parity.
- Finalize Rust architecture.
- Define contracts.
- Create implementation roadmap.

## Phase 2 — Rust Foundation

- Create Rust workspace.
- Implement CLI.
- Implement configuration.
- Implement SQLite storage.
- Implement typed task models.
- Implement logging and errors.

## Phase 3 — Herdr Integration

- Inspect actual Herdr interfaces.
- Implement runtime adapter.
- Connect to existing Herdr.
- Discover workspaces.
- Create and inspect panes.
- Launch agents.
- Monitor sessions.
- Receive runtime events.

## Phase 4 — Agent Management

- Implement discovery.
- Implement registry.
- Implement role configuration.
- Implement launch configuration.
- Implement authentication checks.
- Test multiple providers.

## Phase 5 — Planning and Task Engine

- Implement Lead planning interface.
- Implement plan validation.
- Implement milestone management.
- Implement dependency graph.
- Implement task state machine.

## Phase 6 — Event Coordinator and Scheduler

- Implement event processing.
- Implement event persistence.
- Implement idempotency.
- Implement atomic task claiming.
- Implement worker allocation.
- Implement parallel scheduling.
- Implement recovery reconciliation.

## Phase 7 — Git Management

- Implement feature branches.
- Implement worktree creation.
- Implement worker isolation.
- Implement commits.
- Implement merge management.
- Implement conflict handling.

## Phase 8 — Verification and Review

- Implement automated checks.
- Implement reviewer execution.
- Implement fix jobs.
- Implement bounded review cycles.
- Implement integration validation.

## Phase 9 — TUI

- Implement navigation.
- Implement onboarding.
- Implement project management.
- Implement agent management.
- Implement workflow visualization.
- Implement execution monitoring.
- Implement settings and reports.

## Phase 10 — Recovery and Hardening

- Crash recovery.
- Graceful shutdown.
- Resume.
- Concurrent execution testing.
- Failure simulation.
- Security review.
- Performance testing.

## Phase 11 — Release

- Build release binaries.
- Installation scripts.
- Linux support initially.
- Documentation.
- Example configurations.
- End-to-end validation.
- Release packaging.

---

# 25. Engineering Rules

Follow these rules throughout implementation.

1. Do not duplicate Herdr functionality.
2. Do not make the Lead responsible for continuous coordination.
3. Do not use LLMs for deterministic scheduling.
4. Do not introduce unnecessary microservices.
5. Do not introduce unnecessary distributed infrastructure.
6. Do not create artificial task dependencies.
7. Do not permit duplicate task assignment.
8. Do not allow agents to modify authoritative orchestration state directly.
9. Do not automatically merge unverified work.
10. Do not create infinite retry loops.
11. Do not lose completed work after a crash.
12. Do not force developers into advanced configuration.
13. Do not tightly couple to one AI provider.
14. Do not build a terminal emulator inside Orchestra.
15. Do not implement features without tests.
16. Do not silently remove existing functionality.
17. Do not assume undocumented Herdr APIs.
18. Do not overengineer the architecture.

Prefer simple, testable, explicit designs.

Every module should have a clear responsibility.

---

# 26. Definition of Done

The Rust implementation is considered successful when a developer can:

1. Install Herdr Orchestra.
2. Verify an existing Herdr installation.
3. Select a software project.
4. Automatically discover Claude Code, Codex, or other supported agents.
5. Configure their roles without manually editing files.
6. Submit a feature request.
7. Receive a structured Lead-generated plan.
8. Review and approve the plan.
9. Start orchestration.
10. Allow the Lead to finish its planning responsibility.
11. Have independent workers execute eligible tasks.
12. Execute independent milestones in parallel.
13. Detect agent completion through reliable runtime and task signals.
14. Automatically unlock dependent milestones.
15. Run verification.
16. Request independent code review.
17. Automatically manage bounded fix cycles.
18. Recover from agent failures.
19. Resume after application crashes.
20. Manage isolated Git worktrees.
21. Integrate verified changes into a feature branch.
22. View the entire execution through the TUI.
23. Inspect execution history and final reports.
24. Approve the final integration into the main branch.

The system must work without the Lead remaining active throughout the run.

The orchestration engine must independently manage the complete lifecycle.

---

# 27. Final Product Definition

Herdr Orchestra is a Rust-based, developer-facing multi-agent software development orchestration platform.

It uses Herdr as its existing terminal and coding-agent runtime while providing an independent orchestration engine for planning, task decomposition, dependency management, deterministic scheduling, event processing, parallel execution, verification, review, recovery, and Git integration.

Its primary interfaces are a CLI and interactive TUI sharing one Rust core.

Its primary goal is to make multiple AI coding agents work together as a coordinated software engineering team, with minimal manual intervention and reliable execution.

**Build a system that developers can trust with real software development workflows, not just an impressive multi-agent demonstration.**