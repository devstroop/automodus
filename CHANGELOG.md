# Changelog

All notable changes to Automodus will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
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
- Refactored from single-purpose application to generic browser automation platform
- Simplified architecture focused on workflow execution
- Updated documentation for new workflow-based approach
- Renamed "flows" terminology to "workflows" throughout codebase

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
