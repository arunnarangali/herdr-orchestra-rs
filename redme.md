# Herdr Orchestra — Rust Edition

This README summarizes the project direction defined in @the_main_plan.md.

Herdr Orchestra is a developer-facing orchestration platform for coordinating multiple AI coding agents in a structured, reliable, and reviewable software development workflow. The design in @the_main_plan.md describes a Rust-based implementation that integrates with Herdr, manages work across agents, tracks dependencies, verifies results, and supports human approval and recovery.

## Project vision

The goal is not a literal Python-to-Rust port. Instead, the project aims to build a production-quality orchestration system that:

- coordinates multiple coding agents
- plans and decomposes work into milestones and tasks
- schedules execution across workers and roles
- manages dependency-aware task execution
- integrates with Herdr as the terminal and runtime backend
- verifies implementation quality through tests, lint, build, and custom checks
- reviews changes before merge
- recovers safely from failures and interruptions
- keeps execution state durable and inspectable

The system is designed to be simple for new developers while still supporting advanced configuration, concurrency, custom agents, and recovery policies.

## Core philosophy

Herdr provides the terminal and coding-agent runtime. Herdr Orchestra provides orchestration and workflow intelligence.

The platform separates responsibilities clearly:

- Herdr owns terminal execution, workspaces, panes, sessions, and agent runtime management
- Orchestra owns planning, scheduling, assignment, verification, review, recovery, Git coordination, and state tracking
- AI agents perform implementation, research, testing, and review tasks
- A deterministic scheduler decides what should run next
- An event coordinator processes state transitions and notifies related subsystems
- Git worktrees and branches keep tasks isolated and reviewable
- Humans provide approval and clarification when needed

## Goals

- Build a modular Rust workspace with clear domain boundaries
- Keep orchestration deterministic and auditable
- Support multiple AI coding agent providers and multiple agent instances
- Use Herdr as the runtime backend rather than reimplementing terminal management
- Keep developer onboarding simple with sensible defaults
- Support advanced configuration for power users
- Ensure safe task isolation, review, and merge controls
- Persist state with durable storage and crash-safe recovery

## Major capabilities

### 1. Planning and orchestration

- lead planning workflow to break work into milestones and tasks
- dependency management between milestones and subtasks
- acceptance criteria and verification requirements per task
- milestone-based execution flow with explicit completion requirements

### 2. Agent management

- discovery of installed coding agents from PATH or configured locations
- support for multiple providers and roles
- role-based assignment such as lead, coder, reviewer, tester, and researcher
- per-agent configuration for model, execution limits, cooldowns, and environment
- readiness checks before task assignment

### 3. Runtime integration

- dedicated adapter layer for Herdr
- workspace and session discovery
- launch, monitoring, and lifecycle management for agent sessions
- event-driven status tracking and completion detection
- replaceable runtime abstraction for future backend support

### 4. Scheduling and execution

- deterministic scheduling independent of an LLM
- parallel task execution for independent work
- atomic task claiming to avoid duplicate assignment
- dependency-aware execution flow
- configurable concurrency limits and retry logic

### 5. Verification and review

- build, test, lint, and type checks
- independent reviewer workflow
- acceptance evaluation after implementation
- review states such as approved, requested changes, or blocked
- safe fix cycles without unbounded review loops

### 6. Git and task isolation

- feature branch and worktree support
- task-level git isolation
- merge conflict detection and recovery
- review-before-merge policy
- cleanup and safe task termination

### 7. Persistence and recovery

- durable SQLite-backed state
- typed task and event models
- transaction-safe state transitions
- recovery after crashes or interrupted execution
- event replay and reconciliation

## Target architecture

The project is organized around a modular Rust application with the following responsibilities:

- Core orchestration engine
- Lead planning interface
- Task and milestone management
- Dependency engine
- Deterministic scheduler
- Event coordinator
- Agent registry and management
- Herdr runtime integration
- Git and worktree management
- Verification and review
- Failure recovery
- Persistent state management
- CLI
- Interactive TUI
- Reporting and observability

## Recommended technology stack

The plan calls for stable Rust and a focused set of libraries:

- Tokio for async execution
- Serde for serialization
- Clap for CLI commands
- Ratatui and Crossterm for TUI
- SQLx with SQLite for persistence
- Tracing for structured logging
- Notify for filesystem-based triggers where needed
- Thiserror and Anyhow for typed and application-level errors
- Tokio process APIs for runtime process management
- Petgraph or similar graph tooling for dependency modeling

## User experience

### CLI

The CLI should provide a clean developer workflow, including commands such as:

- orch init
- orch doctor
- orch agents
- orch plan
- orch start
- orch status
- orch tasks
- orch approve
- orch pause
- orch resume
- orch retry
- orch logs
- orch report
- orch tui

### TUI

The interactive terminal user interface should allow developers to:

- monitor current project state
- inspect active orchestration
- view worker and agent availability
- review tasks and dependencies
- inspect execution logs and verification data
- approve or review work
- adjust runtime, scheduling, and settings

## Minimum onboarding experience

The application is intended to be useable with a minimal setup:

1. install Herdr Orchestra
2. verify the Herdr runtime
3. select a project directory
4. discover installed agents
5. assign default roles
6. start orchestration with sensible defaults

Advanced users can then add custom agents, model options, execution limits, verification commands, and project-specific policies.

## Workflow model

The intended execution flow is:

1. Project and repository setup
2. Lead planning and milestone generation
3. Plan validation and approval
4. Scheduler assigns tasks based on readiness and dependencies
5. Agents execute implementation work in isolated task contexts
6. Verification checks run automatically
7. Reviewers evaluate results independently
8. Fixes are assigned when needed
9. Approved changes are merged with human oversight
10. Execution state and reports are persisted for later inspection

## Storage and state model

The project proposes a durable state directory similar to:

- .orchestrate/config.toml
- .orchestrate/plan.md
- .orchestrate/state.db
- .orchestrate/events.jsonl
- .orchestrate/logs/
- .orchestrate/reports/
- .orchestrate/tasks/
- .orchestrate/worktrees/

SQLite should act as the authoritative runtime state store, while human-readable files remain useful for inspection and manual intervention.

## Key design principles

- do not duplicate the Herdr runtime
- do not assume one provider or one agent is the only valid worker
- prefer explicit contracts over hidden coupling
- keep the scheduler independent from model decisions
- use typed state transitions rather than uncontrolled mutation
- support recovery and safe resume rather than fragile execution
- keep onboarding simple while maintaining advanced configuration paths

## Status

This project is currently planned as a Rust implementation of the Herdr Orchestra orchestration system, as defined in @the_main_plan.md.

The focus is on building a reliable orchestration engine around the Herdr runtime model, without treating this as a simple line-by-line translation of prior work.

## Source specification

This README is a condensed summary of the detailed implementation guidance and architecture described in @the_main_plan.md.

## License

This project does not yet specify a license in the plan. Add or confirm the project license before publishing or distributing the software.
