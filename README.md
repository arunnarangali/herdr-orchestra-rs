# Herdr Orchestra — Rust Edition

Herdr Orchestra is a Rust-based, multi-agent software development orchestrator. It coordinates AI coding agents, tracks work and dependencies, and aims to make agent-driven development reliable, reviewable, and recoverable.

Orchestra uses [Herdr](https://github.com/arunnarangali/herdr) as its terminal and coding-agent runtime. Herdr manages workspaces, panes, and agent processes; Orchestra manages plans, task assignment, scheduling, verification, review, recovery, and Git workflows. Orchestra is not a terminal emulator or terminal multiplexer.

The Rust edition is intended to be a drop-in successor to the Python `herdr-orchestra` 4.x release: preserving its command names, plan format, and `.orchestrate/` project data where possible, while improving the implementation and adding the planned interfaces.

## Project status

This repository is under active development and is **not production-ready**. The current checkout contains the Rust workspace foundation and a preliminary `orch` CLI. Most orchestration commands are not implemented yet. The desktop GUI and local REST API are planned, but are not available in this checkout.

The target product has two ways to interact with the same Rust core:

- **CLI** for developers, scripts, and coding agents running in Herdr.
- **Desktop GUI** built with Tauri for monitoring and controlling runs. It will communicate through a local REST API; the API is also intended to be usable directly by other tools.

The current plan calls for a GUI rather than a TUI. The CLI, API, and GUI will share one core; the GUI and API will send requests to the run's event manager rather than acting as a second driver. The GUI must not recreate Herdr's terminal panes; it will present available agent output and provide a way to open the corresponding Herdr workspace.

The planned run lifecycle has an AI lead create and dispatch a plan, then pause. A non-AI event manager monitors the work, advances ready tasks, and wakes the lead when a decision is needed. The plan also covers agent and model management, dependency-aware scheduling, Git worktrees, verification, independent review, recovery, and reporting.

## Workspace

The Rust workspace currently contains:

- `crates/orch` — the `orch` command-line application.
- `crates/orch-core` — shared orchestration primitives, state, storage, and runtime support.
- `crates/orch-config` — configuration models and defaults.

See the [implementation plan](docs/plan.md) for the detailed architecture, compatibility requirements, command behavior, and roadmap.

## Requirements

- Stable Rust toolchain (Rust 2024 edition).
- Cargo.
- Herdr and supported coding-agent CLIs will be required for end-to-end orchestration as those integrations are implemented.

## Build and test

From the repository root:

```sh
cargo build --workspace
cargo test --workspace
```

Run the current CLI:

```sh
cargo run -p orch -- --help
cargo run -p orch -- --version
```

The CLI currently provides help and version output. Other recognized orchestration commands return an explicit “not implemented in this build” message until their implementation is added.

## Desktop app

The GUI in `gui/` is a React + TypeScript (strict) app built with Vite. It only reads and sends
requests through the local API of `orch serve` (`/api/v1`); it never drives a run itself. It
needs Node 20 or newer.

In a browser (two commands, then open http://127.0.0.1:5173):

```sh
cd gui && npm ci && npm run dev
```

Start the API in a project and allow the dev page to call it, then enter the address `orch serve`
prints and the token from the file it names (`~/.config/herdr-orchestra/api-token`). The token is
kept in memory only.

```sh
orch serve --port 8787 --allow-origin http://127.0.0.1:5173
```

Other scripts: `npm run typecheck`, `npm test` (Vitest + Testing Library), `npm run build`.

The desktop window is a Tauri 2 shell in `gui/src-tauri/` (outside the Cargo workspace, so
`cargo build --workspace` does not need the desktop libraries; on Linux it needs webkit2gtk 4.1).
It lets you choose a project folder, starts `orch serve` there, hands the page its address and
token, and stops the server when the window closes. It finds `orch` through `ORCH_BIN` or the PATH
(and Herdr through `HERDR_BIN` or the PATH).

```sh
cargo check --manifest-path gui/src-tauri/Cargo.toml
cd gui && npm run tauri dev
```

**API contract.** `crates/orch-api/tests/contract.rs` builds every response shape from the
server's real types and compares it with the JSON committed in `gui/src/api/contract/`; the
GUI's `npm run typecheck` fails when those samples stop fitting `gui/src/api/types.ts`. After a
deliberate API change run `ORCH_UPDATE_CONTRACT=1 cargo test -p orch-api --test contract`, then
update the TypeScript types the typecheck points at.

## Development

Format and lint the workspace with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

Project data and runtime files are intended to live in a repository-local `.orchestrate/` directory. Do not rely on the planned configuration or runtime behavior until it is implemented and documented by the CLI.

## License

This project is licensed under the [MIT License](LICENSE).
