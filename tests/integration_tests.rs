//! Integration Tests for Automodus
//!
//! These tests verify the API endpoints and workflow execution.
//!
//! ## Running Tests
//!
//! Unit tests (no server required):
//! ```bash
//! cargo test --lib
//! ```
//!
//! Integration tests (requires server running):
//! ```bash
//! # Terminal 1: Start server
//! cargo run -- serve
//!
//! # Terminal 2: Run integration tests
//! cargo test --test integration_tests -- --ignored
//! ```

use anyhow::Result;
use reqwest::Client;
use serde_json::{json, Value};

const BASE_URL: &str = "http://localhost:3000";

// ============================================================================
// Health Endpoint Tests
// ============================================================================

#[cfg(test)]
mod health_tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_health_endpoint() -> Result<()> {
        let client = Client::new();

        let response = client
            .get(&format!("{}/api/health", BASE_URL))
            .send()
            .await?;

        assert!(
            response.status().is_success(),
            "Health check should succeed"
        );

        let body: Value = response.json().await?;

        // Verify response structure
        assert_eq!(body["status"], "ok", "Status should be 'ok'");
        assert!(body["version"].is_string(), "Version should be a string");
        assert!(
            body["workflows_loaded"].is_number(),
            "workflows_loaded should be a number"
        );
        assert!(
            body["browser_running"].is_boolean(),
            "browser_running should be a boolean"
        );

        println!("Health response: {}", serde_json::to_string_pretty(&body)?);
        Ok(())
    }
}

// ============================================================================
// Workflow Endpoint Tests
// ============================================================================

#[cfg(test)]
mod workflow_tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_list_workflows() -> Result<()> {
        let client = Client::new();

        let response = client
            .get(&format!("{}/api/workflows", BASE_URL))
            .send()
            .await?;

        assert!(
            response.status().is_success(),
            "List workflows should succeed"
        );

        let body: Value = response.json().await?;

        // Verify response structure
        assert!(body["workflows"].is_array(), "Should have workflows array");

        let workflows = body["workflows"].as_array().unwrap();
        println!("Found {} workflows", workflows.len());

        for workflow in workflows {
            assert!(workflow["name"].is_string(), "Workflow should have name");
            assert!(
                workflow["steps"].is_number(),
                "Workflow should have steps count"
            );
            assert!(
                workflow["params"].is_array(),
                "Workflow should have params array"
            );
            println!("  - {}", workflow["name"]);
        }

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_get_workflow_by_name() -> Result<()> {
        let client = Client::new();

        // First, list workflows to get a name
        let list_response = client
            .get(&format!("{}/api/workflows", BASE_URL))
            .send()
            .await?;

        let list_body: Value = list_response.json().await?;
        let workflows = list_body["workflows"].as_array().unwrap();

        if workflows.is_empty() {
            println!("No workflows loaded, skipping get workflow test");
            return Ok(());
        }

        let workflow_name = workflows[0]["name"].as_str().unwrap();
        println!("Testing get workflow: {}", workflow_name);

        // Get specific workflow
        let response = client
            .get(&format!("{}/api/workflows/{}", BASE_URL, workflow_name))
            .send()
            .await?;

        assert!(
            response.status().is_success(),
            "Get workflow should succeed"
        );

        let body: Value = response.json().await?;
        assert_eq!(body["name"], workflow_name, "Name should match");
        println!("Workflow details: {}", serde_json::to_string_pretty(&body)?);

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_get_nonexistent_workflow() -> Result<()> {
        let client = Client::new();

        let response = client
            .get(&format!(
                "{}/api/workflows/nonexistent_workflow_xyz",
                BASE_URL
            ))
            .send()
            .await?;

        assert_eq!(
            response.status(),
            404,
            "Should return 404 for nonexistent workflow"
        );

        let body: Value = response.json().await?;
        assert!(body["error"].is_string(), "Should have error message");
        println!("Expected error: {}", body["error"]);

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_reload_workflows() -> Result<()> {
        let client = Client::new();

        let response = client
            .post(&format!("{}/api/workflows/reload", BASE_URL))
            .send()
            .await?;

        assert!(
            response.status().is_success(),
            "Reload workflows should succeed"
        );

        let body: Value = response.json().await?;

        assert!(body["success"].is_boolean(), "Should have success field");
        assert!(body["count"].is_number(), "Should have count field");
        assert!(body["message"].is_string(), "Should have message field");

        println!(
            "Reload result: {} workflows - {}",
            body["count"], body["message"]
        );

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires running server (and browser)
    async fn test_run_workflow() -> Result<()> {
        let client = Client::new();

        // First, list workflows to find one to run
        let list_response = client
            .get(&format!("{}/api/workflows", BASE_URL))
            .send()
            .await?;

        let list_body: Value = list_response.json().await?;
        let workflows = list_body["workflows"].as_array().unwrap();

        if workflows.is_empty() {
            println!("No workflows loaded, skipping run workflow test");
            return Ok(());
        }

        let workflow_name = workflows[0]["name"].as_str().unwrap();
        println!("Testing run workflow: {}", workflow_name);

        // Run the workflow
        let response = client
            .post(&format!("{}/api/workflows/{}/run", BASE_URL, workflow_name))
            .json(&json!({
                "params": {}
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        // Check response structure (may fail if browser not available)
        assert!(body["success"].is_boolean(), "Should have success field");
        assert!(
            body["workflow_name"].is_string(),
            "Should have workflow_name field"
        );
        assert!(
            body["duration_ms"].is_number(),
            "Should have duration_ms field"
        );
        assert!(
            body["steps_executed"].is_number(),
            "Should have steps_executed field"
        );

        println!(
            "Run result: success={}, steps={}, duration={}ms",
            body["success"], body["steps_executed"], body["duration_ms"]
        );

        if let Some(error) = body["error"].as_str() {
            println!("Error (may be expected if browser unavailable): {}", error);
        }

        Ok(())
    }
}

// ============================================================================
// Browser Endpoint Tests
// ============================================================================

#[cfg(test)]
mod browser_tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires running server with browser
    async fn test_browser_goto() -> Result<()> {
        let client = Client::new();

        let response = client
            .post(&format!("{}/api/browser/goto", BASE_URL))
            .json(&json!({
                "url": "https://example.com"
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        assert!(body["success"].is_boolean(), "Should have success field");
        assert!(body["url"].is_string(), "Should have url field");

        if body["success"].as_bool().unwrap_or(false) {
            println!("Navigation successful: {}", body["url"]);
        } else {
            println!(
                "Navigation failed (may be expected): {}",
                body["error"].as_str().unwrap_or("unknown error")
            );
        }

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires running server with browser
    async fn test_browser_screenshot() -> Result<()> {
        let client = Client::new();

        let response = client
            .get(&format!("{}/api/browser/screenshot", BASE_URL))
            .send()
            .await?;

        if response.status().is_success() {
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            assert!(
                content_type.contains("image/png") || content_type.contains("application/json"),
                "Should return PNG image or JSON error"
            );

            let bytes = response.bytes().await?;
            println!("Screenshot response: {} bytes", bytes.len());
        } else {
            println!(
                "Screenshot failed (may be expected if browser not running): {}",
                response.status()
            );
        }

        Ok(())
    }
}

// ============================================================================
// Concurrent Request Tests
// ============================================================================

#[cfg(test)]
mod concurrency_tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_concurrent_health_requests() -> Result<()> {
        let client = Client::new();

        // Send multiple concurrent requests
        let mut handles = vec![];

        for i in 0..10 {
            let client = client.clone();
            let handle = tokio::spawn(async move {
                let response = client
                    .get(&format!("{}/api/health", BASE_URL))
                    .send()
                    .await?;

                let status = response.status();
                Ok::<_, anyhow::Error>((i, status))
            });
            handles.push(handle);
        }

        // Wait for all requests
        let mut success_count = 0;
        for handle in handles {
            let (i, status) = handle.await??;
            if status.is_success() {
                success_count += 1;
            }
            println!("Request {} completed with status: {}", i, status);
        }

        assert_eq!(success_count, 10, "All concurrent requests should succeed");
        println!("Concurrent requests test completed successfully");

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_concurrent_workflow_list_requests() -> Result<()> {
        let client = Client::new();

        let mut handles = vec![];

        for i in 0..5 {
            let client = client.clone();
            let handle = tokio::spawn(async move {
                let response = client
                    .get(&format!("{}/api/workflows", BASE_URL))
                    .send()
                    .await?;

                let status = response.status();
                let body: Value = response.json().await?;
                let count = body["workflows"].as_array().map(|v| v.len()).unwrap_or(0);

                Ok::<_, anyhow::Error>((i, status, count))
            });
            handles.push(handle);
        }

        for handle in handles {
            let (i, status, count) = handle.await??;
            assert!(status.is_success(), "Request {} should succeed", i);
            println!("Request {} found {} workflows", i, count);
        }

        Ok(())
    }
}

// ============================================================================
// API Error Handling Tests
// ============================================================================

#[cfg(test)]
mod error_tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_invalid_endpoint() -> Result<()> {
        let client = Client::new();

        let response = client
            .get(&format!("{}/api/invalid_endpoint", BASE_URL))
            .send()
            .await?;

        assert!(
            response.status().is_client_error(),
            "Invalid endpoint should return 4xx"
        );
        println!("Invalid endpoint returned: {}", response.status());

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_invalid_json_body() -> Result<()> {
        let client = Client::new();

        let response = client
            .post(&format!("{}/api/browser/goto", BASE_URL))
            .header("content-type", "application/json")
            .body("{ invalid json }")
            .send()
            .await?;

        assert!(
            response.status().is_client_error(),
            "Invalid JSON should return 4xx"
        );
        println!("Invalid JSON returned: {}", response.status());

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_missing_required_field() -> Result<()> {
        let client = Client::new();

        // Send goto request without url field
        let response = client
            .post(&format!("{}/api/browser/goto", BASE_URL))
            .json(&json!({}))
            .send()
            .await?;

        assert!(
            response.status().is_client_error(),
            "Missing required field should return 4xx"
        );
        println!("Missing field returned: {}", response.status());

        Ok(())
    }
}
