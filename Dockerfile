# Multi-stage Dockerfile for Herdr Orchestra (Rust Edition)
# Stage 1: Build the React/Vite GUI
FROM node:22-bookworm-slim AS gui-builder
WORKDIR /app/gui
COPY gui/package*.json ./
RUN npm ci
COPY gui/ ./
RUN npm run build

# Stage 2: Build the Rust CLI & Core Workspace
FROM rust:latest AS rust-builder
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
RUN cargo build --release --workspace

# Stage 3: Minimal runtime container
FROM debian:bookworm-slim AS runtime

# Install basic runtime dependencies (git is required for git-based worktrees & VCS operations)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

# Copy compiled binaries from builder stages
COPY --from=rust-builder /app/target/release/orch /usr/local/bin/orch
COPY --from=gui-builder /app/gui/dist /var/www/herdr-orchestra-gui

# Configuration & data directory
ENV XDG_CONFIG_HOME=/root/.config

# Default port for `orch serve`
EXPOSE 8787

# Entrypoint default to the orch CLI
ENTRYPOINT ["orch"]
CMD ["--help"]
