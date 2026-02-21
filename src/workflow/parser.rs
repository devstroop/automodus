//! Workflow Parser
//!
//! Parses YAML files into Workflow definitions.

use anyhow::{Context, Result};
use std::path::Path;

use super::schema::Workflow;

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
    pub fn validate(workflow: &Workflow) -> Result<()> {
        // Name is required
        if workflow.name.trim().is_empty() {
            anyhow::bail!("Workflow name is required");
        }

        // Must have at least one step
        if workflow.steps.is_empty() {
            anyhow::bail!("Workflow must have at least one step");
        }

        // Validate each step
        for (idx, step) in workflow.steps.iter().enumerate() {
            Self::validate_step(step, idx)?;
        }

        // Validate triggers
        Self::validate_triggers(&workflow.triggers)?;

        Ok(())
    }

    fn validate_step(step: &super::schema::Step, index: usize) -> Result<()> {
        // Action is required
        if step.action.trim().is_empty() {
            anyhow::bail!("Step {} has no action specified", index);
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
                if !step.params.contains_key("steps") {
                    anyhow::bail!("Step {} (loop): 'steps' parameter is required", index);
                }
            }
            "condition" => {
                if !step.params.contains_key("if") {
                    anyhow::bail!("Step {} (condition): 'if' parameter is required", index);
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
            // Actions without required params
            "screenshot" | "wait_for" | "eval" | "http" => {}
            "tab.new" | "tab.switch" | "tab.close" => {}
            "back" | "forward" | "reload" => {}
            // HTTP actions
            "http.get" | "http.post" | "http.put" | "http.patch" | "http.delete" | "http.request" => {}
            // Utility actions
            "log" | "debug" | "print" => {}
            "wait_upload" => {}
            // Unknown actions - warn but don't fail (could be custom)
            _ => {
                tracing::warn!("Unknown action '{}' at step {}", step.action, index);
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
}
