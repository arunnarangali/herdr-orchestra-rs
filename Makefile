.DEFAULT_GOAL := help
SHELL := /bin/bash

# Configuration variables
CARGO          ?= cargo
NPM            ?= npm
DOCKER         ?= docker
PORT           ?= 8787
GUI_PORT       ?= 5173
HOST           ?= 127.0.0.1
IMAGE_NAME     ?= herdr-orchestra:latest
CONTAINER_NAME ?= herdr-orchestra-app

.PHONY: help build build-release build-gui build-all test test-rust test-gui check check-tauri lint clippy fmt fmt-check serve dev-gui dev-tauri run-desktop status sync doctor models agents docker-build docker-run docker-compose-up clean

## Display this help message
help: ## Display this help message
	@echo "Herdr Orchestra — Build & Automation Commands"
	@echo "============================================="
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

build: ## Build Rust workspace (orch CLI + crates)
	$(CARGO) build --workspace

build-release: ## Build Rust workspace in release mode
	$(CARGO) build --workspace --release

build-gui: ## Build React/Vite GUI frontend
	cd gui && $(NPM) run build

build-all: ## Build all components (Rust release + GUI)
	$(CARGO) build --workspace --release
	cd gui && $(NPM) run build

test: ## Run all tests (Rust workspace + GUI suite)
	$(CARGO) test --workspace
	cd gui && $(NPM) test

test-rust: ## Run Rust workspace unit and integration tests
	$(CARGO) test --workspace

test-gui: ## Run GUI frontend tests (Vitest)
	cd gui && $(NPM) test

check: ## Type-check Rust workspace and GUI TypeScript
	$(CARGO) check --workspace
	cd gui && $(NPM) run typecheck

check-tauri: ## Check Tauri desktop application compilation
	$(CARGO) check --manifest-path gui/src-tauri/Cargo.toml

clippy: ## Run clippy linter on Rust workspace
	$(CARGO) clippy --workspace --all-targets

fmt-check: ## Check formatting of Rust code
	$(CARGO) fmt --all -- --check

fmt: ## Format Rust codebase
	$(CARGO) fmt --all

lint: ## Run all linters (fmt-check + clippy)
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets

serve: ## Start local orch REST API server
	$(CARGO) run -p orch -- serve --host $(HOST) --port $(PORT) --allow-origin http://$(HOST):$(GUI_PORT)

dev-gui: ## Start Vite GUI development server
	cd gui && $(NPM) run dev

dev-tauri: ## Start Tauri desktop GUI app in dev mode
	export ORCH_BIN="$${ORCH_BIN:-$(PWD)/target/release/orch}" && cd gui && $(NPM) run tauri dev

run-desktop: ## Build the latest orch + desktop app, then open the desktop app window
	@echo "Building orch (release)..."
	$(CARGO) build --release -p orch
	@echo "Building the desktop app (frontend + shell)..."
	cd gui && $(NPM) run tauri -- build --no-bundle
	@echo "Launching Herdr Orchestra Desktop GUI..."
	@export ORCH_BIN="$${ORCH_BIN:-$(PWD)/target/release/orch}" && ./gui/src-tauri/target/release/herdr-orchestra-gui

status: ## Check current orchestration run status
	$(CARGO) run -p orch -- status

sync: ## Sync .orchestrate/plan.md into state.db
	$(CARGO) run -p orch -- sync

doctor: ## Run diagnostic checks on installed coding agents
	$(CARGO) run -p orch -- doctor

models: ## List available AI models and their tiers
	$(CARGO) run -p orch -- models

agents: ## List registered coding agent kinds
	$(CARGO) run -p orch -- agents

docker-build: ## Build Docker container image
	$(DOCKER) build -t $(IMAGE_NAME) .

docker-run: ## Run Docker container (with host network)
	$(DOCKER) run --rm -it --network host \
		-v "$(PWD)":/workspace \
		-v "$$HOME/.config/herdr-orchestra:/root/.config/herdr-orchestra" \
		$(IMAGE_NAME)

docker-compose-up: ## Start services using docker-compose
	docker compose up --build

clean: ## Clean build artifacts (target directory and gui/dist)
	$(CARGO) clean
	rm -rf gui/dist
