//! HTTP Request Actions
//!
//! Actions for making HTTP requests within workflows.

use crate::actions::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};
use crate::modules::http::client::HttpClient;
use async_trait::async_trait;
use reqwest::Method;
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, info};

/// Helper to extract common parameters from action params
fn extract_request_params(
    params: &HashMap<String, serde_yaml::Value>,
) -> Result<(String, Option<HashMap<String, String>>, Option<String>), ActionError> {
    // URL is required
    let url = params
        .get("url")
        .and_then(|v| match v {
            serde_yaml::Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| ActionError::MissingParameter("url".into()))?;

    // Headers are optional
    let headers = params.get("headers").and_then(|v| {
        if let serde_yaml::Value::Mapping(m) = v {
            let mut headers = HashMap::new();
            for (k, v) in m {
                if let (serde_yaml::Value::String(key), serde_yaml::Value::String(val)) = (k, v) {
                    headers.insert(key.clone(), val.clone());
                }
            }
            if headers.is_empty() {
                None
            } else {
                Some(headers)
            }
        } else {
            None
        }
    });

    // store_as is optional
    let store_as = params.get("store_as").and_then(|v| match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        _ => None,
    });

    Ok((url, headers, store_as))
}

/// Extract JSON body from params
fn extract_body(params: &HashMap<String, serde_yaml::Value>) -> Option<Value> {
    params.get("body").and_then(|v| {
        // Convert serde_yaml::Value to serde_json::Value
        serde_json::to_value(v).ok()
    })
}

/// Convert HttpResponse to ActionOutput
fn response_to_output(
    response: crate::modules::http::client::HttpResponse,
    store_as: Option<String>,
) -> ActionOutput {
    let data = serde_json::to_value(&response).unwrap_or(Value::Null);

    if let Some(key) = store_as {
        // Store the full response
        ActionOutput::store(key, data)
    } else {
        ActionOutput::with_data(data)
    }
}

/// HTTP GET Action
///
/// Makes an HTTP GET request and optionally stores the response.
///
/// ## Parameters
///
/// - `url` (required): URL to request
/// - `headers` (optional): Map of headers to include
/// - `store_as` (optional): Variable name to store response
///
/// ## Example
///
/// ```yaml
/// - action: http.get
///   url: "https://api.example.com/users"
///   headers:
///     Authorization: "Bearer {{params.token}}"
///   store_as: users
/// ```
pub struct HttpGetAction;

#[async_trait]
impl Action for HttpGetAction {
    fn name(&self) -> &'static str {
        "http.get"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        _browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let (url, headers, store_as) = extract_request_params(params)?;

        debug!("HTTP GET: {}", url);

        let client = HttpClient::new().map_err(|e| ActionError::Internal(e.to_string()))?;

        let response = client
            .get(&url, headers)
            .await
            .map_err(|e| ActionError::Internal(e.to_string()))?;

        info!("HTTP GET {} => {}", url, response.status);

        Ok(response_to_output(response, store_as))
    }
}

/// HTTP POST Action
///
/// Makes an HTTP POST request with JSON body.
///
/// ## Parameters
///
/// - `url` (required): URL to request
/// - `body` (optional): JSON body to send
/// - `headers` (optional): Map of headers to include
/// - `store_as` (optional): Variable name to store response
///
/// ## Example
///
/// ```yaml
/// - action: http.post
///   url: "https://api.example.com/data"
///   body:
///     name: "{{params.name}}"
///     value: 42
///   store_as: result
/// ```
pub struct HttpPostAction;

#[async_trait]
impl Action for HttpPostAction {
    fn name(&self) -> &'static str {
        "http.post"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        _browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let (url, headers, store_as) = extract_request_params(params)?;
        let body = extract_body(params);

        debug!("HTTP POST: {} body={:?}", url, body);

        let client = HttpClient::new().map_err(|e| ActionError::Internal(e.to_string()))?;

        let response = client
            .post(&url, headers, body)
            .await
            .map_err(|e| ActionError::Internal(e.to_string()))?;

        info!("HTTP POST {} => {}", url, response.status);

        Ok(response_to_output(response, store_as))
    }
}

/// HTTP PUT Action
///
/// Makes an HTTP PUT request with JSON body.
///
/// ## Parameters
///
/// - `url` (required): URL to request
/// - `body` (optional): JSON body to send
/// - `headers` (optional): Map of headers to include
/// - `store_as` (optional): Variable name to store response
pub struct HttpPutAction;

#[async_trait]
impl Action for HttpPutAction {
    fn name(&self) -> &'static str {
        "http.put"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        _browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let (url, headers, store_as) = extract_request_params(params)?;
        let body = extract_body(params);

        debug!("HTTP PUT: {} body={:?}", url, body);

        let client = HttpClient::new().map_err(|e| ActionError::Internal(e.to_string()))?;

        let response = client
            .put(&url, headers, body)
            .await
            .map_err(|e| ActionError::Internal(e.to_string()))?;

        info!("HTTP PUT {} => {}", url, response.status);

        Ok(response_to_output(response, store_as))
    }
}

/// HTTP PATCH Action
///
/// Makes an HTTP PATCH request with JSON body.
///
/// ## Parameters
///
/// - `url` (required): URL to request
/// - `body` (optional): JSON body to send
/// - `headers` (optional): Map of headers to include
/// - `store_as` (optional): Variable name to store response
pub struct HttpPatchAction;

#[async_trait]
impl Action for HttpPatchAction {
    fn name(&self) -> &'static str {
        "http.patch"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        _browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let (url, headers, store_as) = extract_request_params(params)?;
        let body = extract_body(params);

        debug!("HTTP PATCH: {} body={:?}", url, body);

        let client = HttpClient::new().map_err(|e| ActionError::Internal(e.to_string()))?;

        let response = client
            .patch(&url, headers, body)
            .await
            .map_err(|e| ActionError::Internal(e.to_string()))?;

        info!("HTTP PATCH {} => {}", url, response.status);

        Ok(response_to_output(response, store_as))
    }
}

/// HTTP DELETE Action
///
/// Makes an HTTP DELETE request.
///
/// ## Parameters
///
/// - `url` (required): URL to request
/// - `headers` (optional): Map of headers to include
/// - `store_as` (optional): Variable name to store response
pub struct HttpDeleteAction;

#[async_trait]
impl Action for HttpDeleteAction {
    fn name(&self) -> &'static str {
        "http.delete"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        _browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let (url, headers, store_as) = extract_request_params(params)?;

        debug!("HTTP DELETE: {}", url);

        let client = HttpClient::new().map_err(|e| ActionError::Internal(e.to_string()))?;

        let response = client
            .delete(&url, headers)
            .await
            .map_err(|e| ActionError::Internal(e.to_string()))?;

        info!("HTTP DELETE {} => {}", url, response.status);

        Ok(response_to_output(response, store_as))
    }
}

/// HTTP Request Action (Generic)
///
/// Makes an HTTP request with any method.
///
/// ## Parameters
///
/// - `method` (required): HTTP method (GET, POST, PUT, PATCH, DELETE, etc.)
/// - `url` (required): URL to request
/// - `body` (optional): JSON body to send
/// - `headers` (optional): Map of headers to include
/// - `store_as` (optional): Variable name to store response
///
/// ## Example
///
/// ```yaml
/// - action: http.request
///   method: OPTIONS
///   url: "https://api.example.com/endpoint"
///   store_as: options_result
/// ```
pub struct HttpRequestAction;

#[async_trait]
impl Action for HttpRequestAction {
    fn name(&self) -> &'static str {
        "http.request"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        _browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let method_str = params
            .get("method")
            .and_then(|v| match v {
                serde_yaml::Value::String(s) => Some(s.to_uppercase()),
                _ => None,
            })
            .ok_or_else(|| ActionError::MissingParameter("method".into()))?;

        let method: Method = method_str.parse().map_err(|_| {
            ActionError::InvalidParameter(format!("Invalid HTTP method: {}", method_str))
        })?;

        let (url, headers, store_as) = extract_request_params(params)?;
        let body = extract_body(params);

        debug!("HTTP {}: {} body={:?}", method, url, body);

        let client = HttpClient::new().map_err(|e| ActionError::Internal(e.to_string()))?;

        let response = client
            .request(method.clone(), &url, headers, body)
            .await
            .map_err(|e| ActionError::Internal(e.to_string()))?;

        info!("HTTP {} {} => {}", method, url, response.status);

        Ok(response_to_output(response, store_as))
    }
}
