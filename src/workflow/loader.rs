//! Workflow Loader
//!
//! Loads and manages workflows from a directory.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::parser::WorkflowParser;
use super::schema::Workflow;

/// Manages loading and caching of workflows
pub struct WorkflowLoader {
    /// Directory containing workflow YAML files
    workflows_dir: PathBuf,
    /// Loaded workflows cache
    workflows: Arc<RwLock<HashMap<String, Workflow>>>,
}

impl WorkflowLoader {
    /// Create a new workflow loader
    pub fn new<P: AsRef<Path>>(workflows_dir: P) -> Self {
        Self {
            workflows_dir: workflows_dir.as_ref().to_path_buf(),
            workflows: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load all workflows from the directory
    pub async fn load_all(&self) -> Result<Vec<String>> {
        let mut loaded = Vec::new();

        // Ensure directory exists
        if !self.workflows_dir.exists() {
            std::fs::create_dir_all(&self.workflows_dir).with_context(|| {
                format!(
                    "Failed to create workflows directory: {:?}",
                    self.workflows_dir
                )
            })?;
            return Ok(loaded);
        }

        // Find all YAML files (two patterns since glob doesn't support brace expansion)
        let patterns = [
            self.workflows_dir.join("**/*.yaml"),
            self.workflows_dir.join("**/*.yml"),
        ];

        for pattern in &patterns {
            let pattern_str = pattern.to_string_lossy();

            for entry in glob::glob(&pattern_str)? {
                match entry {
                    Ok(path) => {
                        // Skip if already loaded (in case same workflow has both .yaml and .yml)
                        let name_check = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

                        if self.workflows.read().await.contains_key(name_check) {
                            continue;
                        }

                        match WorkflowParser::parse_file(&path) {
                            Ok(workflow) => {
                                let name = workflow.name.clone();
                                self.workflows.write().await.insert(name.clone(), workflow);
                                loaded.push(name);
                                tracing::info!(
                                    "Loaded workflow: {} from {:?}",
                                    loaded.last().unwrap(),
                                    path
                                );
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse workflow {:?}: {}", path, e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to read path: {}", e);
                    }
                }
            }
        }

        tracing::info!(
            "Loaded {} workflows from {:?}",
            loaded.len(),
            self.workflows_dir
        );
        Ok(loaded)
    }

    /// Load a single workflow by name
    pub async fn load(&self, name: &str) -> Result<Workflow> {
        // Check cache first
        if let Some(workflow) = self.workflows.read().await.get(name) {
            return Ok(workflow.clone());
        }

        // Try to find and load the workflow file
        let yaml_path = self.workflows_dir.join(format!("{}.yaml", name));
        let yml_path = self.workflows_dir.join(format!("{}.yml", name));

        let path = if yaml_path.exists() {
            yaml_path
        } else if yml_path.exists() {
            yml_path
        } else {
            anyhow::bail!("Workflow '{}' not found in {:?}", name, self.workflows_dir);
        };

        let workflow = WorkflowParser::parse_file(&path)?;

        // Cache it
        self.workflows
            .write()
            .await
            .insert(name.to_string(), workflow.clone());

        Ok(workflow)
    }

    /// Get a workflow from cache
    pub async fn get(&self, name: &str) -> Option<Workflow> {
        self.workflows.read().await.get(name).cloned()
    }

    /// List all loaded workflow names
    pub async fn list(&self) -> Vec<String> {
        self.workflows.read().await.keys().cloned().collect()
    }

    /// Reload a specific workflow
    pub async fn reload(&self, name: &str) -> Result<()> {
        // Remove from cache
        self.workflows.write().await.remove(name);

        // Reload
        self.load(name).await?;
        Ok(())
    }

    /// Reload all workflows
    pub async fn reload_all(&self) -> Result<Vec<String>> {
        // Clear cache
        self.workflows.write().await.clear();

        // Reload all
        self.load_all().await
    }

    /// Add a workflow programmatically
    pub async fn add(&self, workflow: Workflow) -> Result<()> {
        let name = workflow.name.clone();
        self.workflows.write().await.insert(name, workflow);
        Ok(())
    }

    /// Remove a workflow
    pub async fn remove(&self, name: &str) -> Option<Workflow> {
        self.workflows.write().await.remove(name)
    }

    /// Save a workflow to file
    pub async fn save(&self, workflow: &Workflow) -> Result<PathBuf> {
        let path = self.workflows_dir.join(format!("{}.yaml", workflow.name));

        let yaml =
            serde_yaml::to_string(workflow).context("Failed to serialize workflow to YAML")?;

        std::fs::write(&path, yaml)
            .with_context(|| format!("Failed to write workflow to {:?}", path))?;

        // Update cache
        self.workflows
            .write()
            .await
            .insert(workflow.name.clone(), workflow.clone());

        Ok(path)
    }

    /// Get workflows directory
    pub fn directory(&self) -> &Path {
        &self.workflows_dir
    }

    /// Get number of loaded workflows
    pub async fn count(&self) -> usize {
        self.workflows.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_load_workflow_from_dir() {
        let dir = tempdir().unwrap();

        // Create a test workflow file
        let workflow_yaml = r#"
name: test-workflow
steps:
  - action: goto
    url: "https://example.com"
"#;
        std::fs::write(dir.path().join("test-workflow.yaml"), workflow_yaml).unwrap();

        let loader = WorkflowLoader::new(dir.path());
        let loaded = loader.load_all().await.unwrap();

        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains(&"test-workflow".to_string()));

        let workflow = loader.get("test-workflow").await.unwrap();
        assert_eq!(workflow.name, "test-workflow");
    }

    #[tokio::test]
    async fn test_add_workflow_programmatically() {
        let dir = tempdir().unwrap();
        let loader = WorkflowLoader::new(dir.path());

        let workflow = Workflow {
            name: "dynamic-workflow".to_string(),
            version: "1.0".to_string(),
            description: None,
            browser: super::super::schema::BrowserConfig::default(),
            triggers: super::super::schema::Triggers::default(),
            params: HashMap::new(),
            vars: None,
            steps: vec![],
            output: None,
            on_complete: None,
            on_error: None,
        };

        loader.add(workflow).await.unwrap();

        let retrieved = loader.get("dynamic-workflow").await;
        assert!(retrieved.is_some());
    }
}
