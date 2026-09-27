# Contributing to Automodus

We welcome contributions to Automodus! This document provides guidelines for contributing to the project.

## Getting Started

### Prerequisites

- **Rust 1.70+** (latest stable recommended)
- **Git** for version control
- A browser for the smoke suite: **Chrome/Chromium**, **Firefox**, or
  **Lightpanda** (CI's engine matrix runs all three; any one is enough locally)
- **Docker** (optional, for containerized development)

### Fork and Clone

1. Fork the repository on GitHub
2. Clone your fork locally (the examples live in a git submodule):
   ```bash
   git clone --recurse-submodules https://github.com/YOUR_USERNAME/automodus.git
   cd automodus
   # (already cloned without submodules? run: git submodule update --init)
   ```

3. Add the upstream remote:
   ```bash
   git remote add upstream https://github.com/devstroop/automodus.git
   ```

## Development Environment

### Quick Setup

```bash
# Install dependencies and build
cargo build

# Run tests
cargo test

# Run a workflow (examples live in the examples/ submodule)
cargo run -- run examples/browser/search_form.yaml

# Validate all example workflows (this is also a CI gate)
cargo run -- validate examples/

# Run the engine smoke suite locally (chromium | firefox | lightpanda)
scripts/smoke.sh chromium

# Start server mode (foreground; prefer 'daemon start' in production)
cargo run -- serve

# Generate documentation
cargo doc --open
```

### Development with Docker

```bash
# Build development image
docker build -t automodus-dev .

# Run with volume mount for development
docker run -v $(pwd):/app -p 3000:3000 automodus-dev
```

## Project Structure

```
src/
├── lib.rs              # Library entry point
├── error.rs            # Error types
├── config.rs           # AppConfig (config/app.toml) loader
├── bin/
│   └── automodus.rs    # CLI binary
├── actions/            # Action registry, aliases, capabilities
├── api/                # REST API (axum): handlers, schemas, server, ws
├── core/               # Core engine
│   ├── engine.rs       # Workflow execution engine
│   ├── app.rs          # AppCore (shared daemon/shell state)
│   ├── context.rs      # Execution context
│   ├── template.rs     # {{...}} interpolation
│   └── json_path.rs    # JSONPath-style extraction
├── daemon/             # Daemon process (mod, config, protocol)
├── shell/              # Interactive shell client
├── modules/
│   ├── browser/        # Browser automation
│   │   ├── session.rs  # SessionAdapter (engine dispatch)
│   │   ├── adapter.rs  # Chromium CDP page adapter
│   │   ├── firefox.rs  # Firefox BiDi backend
│   │   ├── driver.rs   # Browser service
│   │   └── actions/    # Browser-specific actions
│   └── http/           # HTTP client module (reqwest)
├── workflow/           # Workflow definitions
│   ├── parser.rs       # Registry-backed validation
│   └── schema.rs       # Workflow types
├── triggers/           # Trigger types (schema-only today)
└── utils/              # Utilities (logging, debug cleanup, trace, ...)

examples/               # Example workflows (git submodule)
scripts/smoke.sh        # Engine-matrix smoke runner (keep docs in sync)
config/                 # Configuration templates
tests/                  # Integration/session/shell/daemon tests
.github/workflows/      # CI (fmt, clippy, tests, smoke matrix)
```

## Development Workflow

### Branch Strategy

- **main** - Stable, production-ready code (protected; PRs merge into it)
- **develop** - Integration branch — open PRs against `develop`
- **feat/…**, **fix/…** - Feature/bugfix branches cut from `develop`

### Commit Message Convention

Imperative subject line, body explains *why* (not a diff rehash):

```
Fix race in selector wait after back-navigation

Back to the real URL before waiting on the new h1; the old check could
match the stale document.

Co-Authored-By: opencode <noreply@opencode.ai>
```

Subject prefixes like `Fix`, `Add`, `Remove`, `Bump`, `Document` are typical;
scope in parentheses (`fix(browser): …`) is fine but not required.

## Testing Guidelines

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_workflow_parsing

# With output
cargo test -- --nocapture

# Example validation gate (what CI runs)
cargo run -- validate examples/

# Engine smoke suite (what CI runs per engine)
scripts/smoke.sh chromium
```

## Code Style

Follow the official Rust Style Gate (same as CI):

```bash
# Check formatting (CI runs the check, not a rewrite)
cargo fmt --all -- --check

# Lint with warnings denied, mcp feature enabled
cargo clippy --features mcp -- -D warnings

# Build the way CI does
cargo build --features mcp
```

## Pull Request Process

### Before Submitting

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --features mcp -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] `cargo run -- validate examples/` passes (if workflow examples changed)
- [ ] Documentation is updated (README/docs match the code)
- [ ] Branch is up-to-date with `develop`

### Review Process

1. **Automated Checks**: All CI checks must pass (fmt, clippy, tests, 3-engine smoke matrix)
2. **Code Review**: At least one maintainer approval required
3. **Merge**: PRs target `develop`; `main` receives the release merges

## Issue Reporting

### Bug Reports

Include:
- Clear description of the bug
- Steps to reproduce
- Expected vs actual behavior
- Environment (OS, Rust version, browser + engine, e.g. Chrome 130 / chromiumoxide / firefox)
- Relevant logs or screenshots

### Feature Requests

Include:
- Feature description
- Use case / motivation
- Proposed implementation (optional)

## Security

**DO NOT** open public issues for security vulnerabilities.
Email security concerns to: security@devstroop.com

---

Thank you for contributing to Automodus!
