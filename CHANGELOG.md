# Changelog

All notable changes to Automodus will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Firefox engine**: `engine = "firefox"` runs WebDriver BiDi directly via rustenium — no geckodriver installation
- **Lightpanda engine**: `engine = "lightpanda"` spawns `lightpanda serve` and drives it over CDP; nightly binary auto-resolved via PATH/`AUTOMODUS_LIGHTPANDA_PATH`/config
- **`loop` action**: iterate templated YAML item lists with `as` item + optional zero-based `index_as`, scoped to an isolated per-iteration context
- **Step-level `on_success` / `on_failure` handlers**: `goto` (jump in the enclosing list), `emit` (rendered payload, error context on failure), `abort`, and `steps`; wired for every step type, failure-handler `goto` recovers the workflow
- **Example smoke suite** (`scripts/smoke.sh`): validates the examples submodule and runs 11 standalone workflows per engine with timeouts, straggler reaping, and orphan checks; run in CI as a chromium/firefox/lightpanda matrix
- **YAML Workflow Engine**: Define browser automation as declarative YAML workflows
- **15+ Built-in Actions**: navigate, click, type, screenshot, wait, upload, and more
- **Multi-trigger Support**: Manual, scheduled (cron), and HTTP webhook triggers
- **CLI Interface**: Run workflows via `automodus run`, validate with `automodus validate`
- **Server Mode**: REST API with `automodus serve` for headless automation
- **Shell Mode**: Interactive browser debugging with `automodus shell`
- **Configurable Locators**: External CSS selectors in TOML for easy maintenance
- **Variable Substitution**: Use `${env.VAR}`, `${locators.section.key}` in workflows
- **Screenshot Capture**: Automatic screenshots with customizable paths
- **Error Handling**: Rich error types with retry guidance

### Changed
- **CI workflow** now triggers on `main`/`develop` (the `master`/`dev` branches it watched no longer existed, so CI never ran) and adds the engine smoke matrix
- **`automodus run` exits 1 on workflow failure** (adapter dropped before `process::exit` so the browser is not orphaned); the interactive shell path is unchanged
- **Condition branches execute full step blocks**: `call`, `loop`, `retry`, `if:`, and handlers now work inside `then`/`else` (nested `call` previously failed with `ActionNotFound`)
- **Validator rejects unknown actions** via the action registry (aliases included) and prints the known-action list; `debug`, `print`, and bare `http` are no longer accepted as valid actions
- Validator recursively checks nested step lists (condition branches, loop bodies, handler steps) and handler `goto` targets; `parallel: true` loops rejected
- Refactored from single-purpose application to generic browser automation platform
- Simplified architecture focused on workflow execution
- Updated documentation for new workflow-based approach
- Renamed "flows" terminology to "workflows" throughout codebase

### Fixed
- **Firefox process leaked on every run**: the spawned child handle was dropped after launch so cleanup relied on an async close that never runs on CLI exit; the child now lives in the shared kill-on-drop guard (last adapter drop or `close_browser` kills and waits), with `wait()` on the launch error path to avoid zombies
- **Step-level `emit.data` was sent raw** instead of being template-rendered
- rustfmt and clippy violations across the crate (CI fmt/clippy gates pass on a clean checkout)

## [0.1.0] - Initial Release

### Added
- Initial Automodus implementation
- Chrome/Chromium browser automation via CDP
- YAML-based workflow definition
- Basic action library
- Docker containerization
- Documentation and examples

---

## Release Process

### Version Numbering

We follow [Semantic Versioning](https://semver.org/):

- **MAJOR**: Incompatible API changes
- **MINOR**: New functionality in a backwards compatible manner
- **PATCH**: Backwards compatible bug fixes

### Release Checklist

- [ ] Update version in `Cargo.toml`
- [ ] Update `CHANGELOG.md` with release notes
- [ ] Ensure all tests pass
- [ ] Update documentation
- [ ] Create GitHub release with binaries
- [ ] Update Docker images
- [ ] Announce release
