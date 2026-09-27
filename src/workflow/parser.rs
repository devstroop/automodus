//! Workflow Parser
//!
//! Parses YAML files into Workflow definitions.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;

use crate::actions::ActionRegistry;

use super::schema::{Step, StepHandler, Workflow};

/// Engine pseudo-actions handled directly by `WorkflowEngine`, not the
/// action registry (`registry.get` returns `None` for them).
const ENGINE_PSEUDO_ACTIONS: [&str; 3] = ["call", "condition", "loop"];

/// Parses YAML workflow definitions
pub struct WorkflowParser;

impl WorkflowParser {
    /// Parse a workflow from YAML string
    pub fn parse(yaml: &str) -> Result<Workflow> {
        let workflow: Workflow =
            serde_yaml::from_str(yaml).context("Failed to parse YAML workflow definition")?;

        Self::validate(&workflow)?;
        Ok(workflow)
    }

    /// Parse a workflow from a file
    pub fn parse_file<P: AsRef<Path>>(path: P) -> Result<Workflow> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read workflow file: {}", path.display()))?;

        let mut workflow = Self::parse(&content)?;

        // Use filename as name if not specified
        if workflow.name.is_empty() {
            if let Some(stem) = path.file_stem() {
                workflow.name = stem.to_string_lossy().to_string();
            }
        }

        Ok(workflow)
    }

    /// Validate a workflow definition
    ///
    /// Fails on unknown actions (registry-backed, aliases included), missing
    /// required params, malformed nested step lists, and handler `goto`
    /// targets that don't exist in the enclosing step list.
    pub fn validate(workflow: &Workflow) -> Result<()> {
        // Name is required
        if workflow.name.trim().is_empty() {
            anyhow::bail!("Workflow name is required");
        }

        // Must have at least one step
        if workflow.steps.is_empty() {
            anyhow::bail!("Workflow must have at least one step");
        }

        // Single source of truth for registered actions (builtins + aliases)
        let registry = ActionRegistry::new();

        Self::validate_steps(&workflow.steps, &registry)?;

        // Validate triggers
        Self::validate_triggers(&workflow.triggers)?;

        Ok(())
    }

    /// Validate a list of steps (top-level or nested inside a handler/branch/loop)
    fn validate_steps(steps: &[Step], registry: &ActionRegistry) -> Result<()> {
        let enclosing_ids: HashSet<&str> = steps.iter().filter_map(|s| s.id.as_deref()).collect();
        for (idx, step) in steps.iter().enumerate() {
            Self::validate_step(step, idx, &enclosing_ids, registry)?;
        }
        Ok(())
    }

    fn validate_step(
        step: &Step,
        index: usize,
        enclosing_ids: &HashSet<&str>,
        registry: &ActionRegistry,
    ) -> Result<()> {
        // Action is required
        if step.action.trim().is_empty() {
            anyhow::bail!("Step {} has no action specified", index);
        }

        // Action must be registered (or be an engine pseudo-action) — this is
        // the closed action set; typos fail at validate time, not runtime.
        if !ENGINE_PSEUDO_ACTIONS.contains(&step.action.as_str())
            && registry.get(&step.action).is_none()
        {
            let mut known: Vec<&str> = registry.list();
            known.extend(ENGINE_PSEUDO_ACTIONS);
            known.sort_unstable();
            anyhow::bail!(
                "Step {} ({}): unknown action '{}' — known actions: {}",
                index,
                step.id.as_deref().unwrap_or("-"),
                step.action,
                known.join(", ")
            );
        }

        // Validate action-specific requirements
        match step.action.as_str() {
            "goto" | "navigate" => {
                if !step.params.contains_key("url") {
                    anyhow::bail!(
                        "Step {} ({}): 'url' parameter is required for goto action",
                        index,
                        step.action
                    );
                }
            }
            "click" => {
                if !step.params.contains_key("selector") {
                    anyhow::bail!("Step {} (click): 'selector' parameter is required", index);
                }
            }
            "type" => {
                if !step.params.contains_key("selector") {
                    anyhow::bail!("Step {} (type): 'selector' parameter is required", index);
                }
                if !step.params.contains_key("text") {
                    anyhow::bail!("Step {} (type): 'text' parameter is required", index);
                }
            }
            "extract" => {
                if !step.params.contains_key("selector") {
                    anyhow::bail!("Step {} (extract): 'selector' parameter is required", index);
                }
                // Accept either 'store_as' or 'as' for storing results
                if !step.params.contains_key("store_as") && !step.params.contains_key("as") {
                    anyhow::bail!(
                        "Step {} (extract): 'store_as' or 'as' parameter is required",
                        index
                    );
                }
            }
            "loop" => {
                if !step.params.contains_key("items") {
                    anyhow::bail!("Step {} (loop): 'items' parameter is required", index);
                }
                if !step.params.contains_key("as") {
                    anyhow::bail!("Step {} (loop): 'as' parameter is required", index);
                }
                if step.params.get("parallel").and_then(|v| v.as_bool()) == Some(true) {
                    anyhow::bail!(
                        "Step {} (loop): parallel loops are not supported — remove 'parallel' or set it to false",
                        index
                    );
                }
                let body = step.params.get("steps").ok_or_else(|| {
                    anyhow::anyhow!("Step {} (loop): 'steps' parameter is required", index)
                })?;
                let nested: Vec<Step> =
                    serde_yaml::from_value(body.clone()).with_context(|| {
                        format!("Step {} (loop): 'steps' must be a list of steps", index)
                    })?;
                Self::validate_steps(&nested, registry)?;
            }
            "condition" => {
                // Note: 'if' key at step level is captured by step.condition (serde rename),
                // not in params. Check both for compatibility.
                let has_if = step.condition.is_some() || step.params.contains_key("if");
                if !has_if {
                    anyhow::bail!("Step {} (condition): 'if' parameter is required", index);
                }
                for branch in ["then", "else"] {
                    if let Some(val) = step.params.get(branch) {
                        let nested: Vec<Step> =
                            serde_yaml::from_value(val.clone()).with_context(|| {
                                format!(
                                    "Step {} (condition): '{}' must be a list of steps",
                                    index, branch
                                )
                            })?;
                        Self::validate_steps(&nested, registry)?;
                    }
                }
                if !step.params.contains_key("then") {
                    anyhow::bail!("Step {} (condition): 'then' parameter is required", index);
                }
            }
            "call" => {
                if !step.params.contains_key("workflow") {
                    anyhow::bail!("Step {} (call): 'workflow' parameter is required", index);
                }
            }
            "emit" => {
                if !step.params.contains_key("event") {
                    anyhow::bail!("Step {} (emit): 'event' parameter is required", index);
                }
            }
            "sleep" => {
                if !step.params.contains_key("duration") && !step.params.contains_key("ms") {
                    anyhow::bail!(
                        "Step {} (sleep): 'duration' or 'ms' parameter is required",
                        index
                    );
                }
            }
            // Actions without required params at validate time
            "screenshot" | "wait_for" | "eval" => {}
            "tab.list" | "tab.new" | "tab.switch" | "tab.close" => {}
            "back" | "forward" | "reload" => {}
            // HTTP actions
            "http.get" | "http.post" | "http.put" | "http.patch" | "http.delete"
            | "http.request" => {}
            // Utility actions
            "log" | "wait_upload" => {}
            // Registered actions without extra required params (hover, select,
            // upload, file_chooser, …) — registry check above already vetted them.
            _ => {}
        }

        // Validate step-level handlers (on_success / on_failure)
        for (kind, handler) in [
            ("on_success", &step.on_success),
            ("on_failure", &step.on_failure),
        ] {
            if let Some(handler) = handler {
                Self::validate_handler(handler, kind, step, index, enclosing_ids, registry)?;
            }
        }

        Ok(())
    }

    fn validate_handler(
        handler: &StepHandler,
        kind: &str,
        step: &Step,
        index: usize,
        enclosing_ids: &HashSet<&str>,
        registry: &ActionRegistry,
    ) -> Result<()> {
        match handler {
            StepHandler::Goto { goto } => {
                if !enclosing_ids.contains(goto.as_str()) {
                    anyhow::bail!(
                        "Step {} ({}): {} handler jumps to '{}' but no step with that id exists in this step list",
                        index,
                        step.action,
                        kind,
                        goto
                    );
                }
            }
            StepHandler::Abort { .. } | StepHandler::Emit { .. } => {}
            StepHandler::Steps { steps: nested } | StepHandler::StepsList(nested) => {
                Self::validate_steps(nested, registry)?;
            }
        }
        Ok(())
    }

    fn validate_triggers(triggers: &super::schema::Triggers) -> Result<()> {
        // Validate API trigger path
        if let Some(ref api) = triggers.api {
            if !api.path.starts_with('/') {
                anyhow::bail!("API trigger path must start with '/'");
            }
        }

        // Validate webhook path
        if let Some(ref webhook) = triggers.webhook {
            if !webhook.path.starts_with('/') {
                anyhow::bail!("Webhook path must start with '/'");
            }
        }

        // Validate cron schedule format (basic check)
        if let Some(ref schedule) = triggers.schedule {
            let parts: Vec<&str> = schedule.split_whitespace().collect();
            if parts.len() < 5 || parts.len() > 6 {
                anyhow::bail!("Invalid cron schedule format: expected 5-6 fields");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_workflow() {
        let yaml = r#"
name: minimal
steps:
  - action: goto
    url: "https://example.com"
"#;
        let workflow = WorkflowParser::parse(yaml).unwrap();
        assert_eq!(workflow.name, "minimal");
        assert_eq!(workflow.steps.len(), 1);
        assert_eq!(workflow.steps[0].action, "goto");
    }

    #[test]
    fn test_parse_invalid_workflow_no_steps() {
        let yaml = r#"
name: invalid
steps: []
"#;
        let result = WorkflowParser::parse(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_workflow_missing_url() {
        let yaml = r#"
name: invalid
steps:
  - action: goto
"#;
        let result = WorkflowParser::parse(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_workflow_with_variables() {
        let yaml = r#"
name: with-vars
vars:
  base_url: "https://example.com"
  timeout: 30
steps:
  - action: goto
    url: "{{vars.base_url}}"
"#;
        let workflow = WorkflowParser::parse(yaml).unwrap();
        let vars = workflow.vars.as_ref().unwrap();
        assert!(vars.contains_key("base_url"));
        assert!(vars.contains_key("timeout"));
    }

    #[test]
    fn test_parse_whatsapp_workflows() {
        use std::fs;

        // List of WhatsApp workflow file paths relative to project root.
        // Examples live in the examples/ git submodule; missing files are
        // skipped (e.g. clones without `git submodule update --init`).
        let workflow_files = [
            "examples/whatsapp/whatsapp.yaml",
            "examples/whatsapp/_common/ensure_ready.yaml",
            "examples/whatsapp/_common/open_chat.yaml",
            "examples/whatsapp/auth/qr_login.yaml",
            "examples/whatsapp/auth/phone_login.yaml",
            "examples/whatsapp/auth/check_status.yaml",
            "examples/whatsapp/auth/logout.yaml",
            "examples/whatsapp/messaging/send.yaml",
            "examples/whatsapp/messaging/send_text.yaml",
            "examples/whatsapp/messaging/send_media.yaml",
            "examples/whatsapp/messaging/send_document.yaml",
            "examples/whatsapp/chat/get_chats.yaml",
            "examples/whatsapp/chat/get_messages.yaml",
            "examples/whatsapp/chat/watch_messages.yaml",
            "examples/whatsapp/chat/navigate.yaml",
        ];

        for file in workflow_files {
            let path = std::path::Path::new(file);
            if path.exists() {
                let content = fs::read_to_string(path)
                    .unwrap_or_else(|e| panic!("Failed to read {}: {}", file, e));

                // Just verify YAML parses into our Workflow struct
                let result: Result<super::super::schema::Workflow, _> =
                    serde_yaml::from_str(&content);
                assert!(
                    result.is_ok(),
                    "Failed to parse {}: {:?}",
                    file,
                    result.err()
                );

                let workflow = result.unwrap();
                assert!(!workflow.name.is_empty(), "{} should have a name", file);
                assert!(!workflow.steps.is_empty(), "{} should have steps", file);
            }
        }
    }

    #[test]
    fn test_reject_unknown_action() {
        let yaml = r##"
name: typo
steps:
  - action: cl1k
    selector: "#x"
"##;
        let err = WorkflowParser::parse(yaml).unwrap_err().to_string();
        assert!(err.contains("unknown action 'cl1k'"), "{}", err);
        assert!(err.contains("known actions"), "{}", err);
    }

    #[test]
    fn test_reject_removed_debug_and_print_actions() {
        // debug/print are engine pseudo-ops that never existed in the registry —
        // they used to validate as "accepted" and then fail at runtime.
        for action in ["debug", "print"] {
            let yaml = format!(
                "name: legacy\nsteps:\n  - action: {}\n    message: \"x\"\n",
                action
            );
            let err = WorkflowParser::parse(&yaml).unwrap_err().to_string();
            assert!(err.contains("unknown action"), "{}: {}", action, err);
        }
    }

    #[test]
    fn test_accept_action_aliases() {
        let yaml = r##"
name: aliased
steps:
  - action: navigate
    url: "https://example.com"
  - action: input
    selector: "#q"
    text: "hi"
"##;
        WorkflowParser::parse(yaml).unwrap();
    }

    #[test]
    fn test_reject_unknown_on_success_goto_target() {
        let yaml = r#"
name: bad-handler
steps:
  - id: a
    action: emit
    event: ok
    on_success:
      goto: nonexistent
"#;
        let err = WorkflowParser::parse(yaml).unwrap_err().to_string();
        assert!(err.contains("no step with that id"), "{}", err);
    }

    #[test]
    fn test_validate_on_success_goto_within_enclosing_list() {
        let yaml = r#"
name: good-handler
steps:
  - id: a
    action: emit
    event: ok
    on_success:
      goto: b
  - id: b
    action: emit
    event: done
"#;
        WorkflowParser::parse(yaml).unwrap();
    }

    #[test]
    fn test_reject_unknown_action_nested_in_condition() {
        let yaml = r##"
name: bad-nested
steps:
  - action: condition
    if: "true"
    then:
      - action: cl1k
        selector: "#x"
"##;
        let err = WorkflowParser::parse(yaml).unwrap_err().to_string();
        assert!(err.contains("unknown action 'cl1k'"), "{}", err);
    }

    #[test]
    fn test_reject_unknown_action_nested_in_loop() {
        let yaml = r#"
name: bad-loop-body
steps:
  - action: loop
    items: ["a"]
    as: item
    steps:
      - action: nope
"#;
        let err = WorkflowParser::parse(yaml).unwrap_err().to_string();
        assert!(err.contains("unknown action 'nope'"), "{}", err);
    }

    #[test]
    fn test_reject_unknown_action_in_handler_steps() {
        let yaml = r#"
name: bad-handler-steps
steps:
  - action: emit
    event: ok
    on_failure:
      steps:
        - action: nope
"#;
        let err = WorkflowParser::parse(yaml).unwrap_err().to_string();
        assert!(err.contains("unknown action 'nope'"), "{}", err);
    }

    #[test]
    fn test_reject_parallel_loop() {
        let yaml = r#"
name: parallel-loop
steps:
  - action: loop
    items: ["a"]
    as: item
    parallel: true
    steps:
      - action: log
        message: "x"
"#;
        let err = WorkflowParser::parse(yaml).unwrap_err().to_string();
        assert!(err.contains("parallel"), "{}", err);
    }

    #[test]
    fn test_reject_malformed_loop_steps() {
        let yaml = r#"
name: bad-loop-steps
steps:
  - action: loop
    items: ["a"]
    as: item
    steps: "not-a-list"
"#;
        let err = WorkflowParser::parse(yaml).unwrap_err().to_string();
        assert!(err.contains("must be a list of steps"), "{}", err);
    }

    #[test]
    fn test_reject_malformed_condition_branch() {
        let yaml = r#"
name: bad-branch
steps:
  - action: condition
    if: "true"
    then: 42
"#;
        assert!(WorkflowParser::parse(yaml).is_err());
    }

    #[test]
    fn test_parse_workflow_with_valid_loop_and_handlers() {
        let yaml = r#"
name: full-featured
steps:
  - id: l
    action: loop
    items: ["a", "b"]
    as: item
    steps:
      - action: emit
        event: each
        data:
          v: "{{vars.item}}"
    on_success:
      goto: done
  - id: fallback
    action: emit
    event: other
    on_failure:
      goto: done
  - id: done
    action: emit
    event: finished
"#;
        let wf = WorkflowParser::parse(yaml).unwrap();
        assert_eq!(wf.steps.len(), 3);
    }
}
