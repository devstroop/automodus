# Contributing to Automodus

We welcome contributions to Automodus! This document provides guidelines for contributing to the project.

## Getting Started

### Prerequisites

- **Rust 1.70+** (latest stable recommended)
- **Chrome/Chromium browser** (for browser automation)
- **Git** for version control
- **Docker** (optional, for containerized development)

### Fork and Clone

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/YOUR_USERNAME/automodus.git
   cd automodus
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

# Run a workflow (examples live in the workspace sibling ../examples/)
cargo run -- run ../examples/browser/search_form.yaml

# Start server mode
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
├── config.rs           # Configuration management
├── bin/
│   └── automodus.rs    # CLI binary
├── browser/            # Browser automation
│   ├── adapter.rs      # Chrome page adapter
│   └── driver.rs       # Browser service
├── core/               # Core engine
│   ├── engine.rs       # Workflow execution engine
│   └── context.rs      # Execution context
├── workflow/           # Workflow definitions
│   ├── parser.rs       # YAML parser
│   └── types.rs        # Workflow types
├── actions/            # Built-in actions
├── triggers/           # Workflow triggers
├── utils/              # Utilities
└── web/                # Web UI (server mode)

../examples/            # Example workflow definitions (workspace sibling, outside this repo)
config/                 # Configuration files
templates/              # HTML templates
tests/                  # Integration tests
```

## Development Workflow

### Branch Strategy

- **main** - Stable, production-ready code
- **develop** - Integration branch for features
- **feature/feature-name** - Feature development
- **bugfix/bug-description** - Bug fixes

### Commit Message Convention

Follow Conventional Commits:

```
<type>[optional scope]: <description>
```

**Types:**
- feat: New feature
- fix: Bug fix
- docs: Documentation changes
- refactor: Code refactoring
- test: Adding or updating tests
- chore: Build process or auxiliary tool changes

**Examples:**
```
feat(actions): add file download action
fix(browser): resolve connection timeout issues
docs(readme): update workflow examples
```

## Testing Guidelines

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_workflow_parsing

# With output
cargo test -- --nocapture
```

## Code Style

Follow the official Rust Style Guide:

```bash
# Format code
cargo fmt

# Lint code
cargo clippy
```

## Pull Request Process

### Before Submitting

- [ ] Code follows style guidelines (cargo fmt and cargo clippy pass)
- [ ] Tests are written and passing (cargo test)
- [ ] Documentation is updated
- [ ] Branch is up-to-date with main

### Review Process

1. **Automated Checks**: All CI checks must pass
2. **Code Review**: At least one maintainer approval required
3. **Testing**: All tests must pass
4. **Merge**: Squash and merge to main

## Issue Reporting

### Bug Reports

Include:
- Clear description of the bug
- Steps to reproduce
- Expected vs actual behavior
- Environment (OS, Rust version, Chrome version)
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
