//! HTTP Client Wrapper
//!
//! Provides a configured HTTP client for making requests.

use reqwest::{Client, Method, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

/// HTTP client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    /// Request timeout in milliseconds
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,

    /// Follow redirects
    #[serde(default = "default_true")]
    pub follow_redirects: bool,

    /// Maximum redirects to follow
    #[serde(default = "default_max_redirects")]
    pub max_redirects: usize,

    /// Default headers for all requests
    #[serde(default)]
    pub default_headers: HashMap<String, String>,

    /// User-Agent header
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
}

fn default_timeout() -> u64 {
    30000
}
fn default_true() -> bool {
    true
}
fn default_max_redirects() -> usize {
    10
}
fn default_user_agent() -> String {
    format!("Automodus/{}", env!("CARGO_PKG_VERSION"))
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout_ms: default_timeout(),
            follow_redirects: true,
            max_redirects: default_max_redirects(),
            default_headers: HashMap::new(),
            user_agent: default_user_agent(),
        }
    }
}

/// HTTP client error
#[derive(Debug, Error)]
pub enum HttpError {
    #[error("Request failed: {0}")]
    Request(String),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Timeout after {0}ms")]
    Timeout(u64),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Response error: {status} - {message}")]
    Response { status: u16, message: String },

    #[error("JSON parse error: {0}")]
    JsonParse(String),

    #[error("Invalid header: {0}")]
    InvalidHeader(String),
}

impl From<reqwest::Error> for HttpError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            HttpError::Timeout(30000)
        } else if e.is_connect() {
            HttpError::Connection(e.to_string())
        } else {
            HttpError::Request(e.to_string())
        }
    }
}

/// HTTP response data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    /// HTTP status code
    pub status: u16,

    /// Status text
    pub status_text: String,

    /// Response headers
    pub headers: HashMap<String, String>,

    /// Response body (parsed as JSON if possible, otherwise as string)
    pub body: Value,

    /// Whether response was successful (2xx)
    pub ok: bool,

    /// Request URL (after redirects)
    pub url: String,
}

/// HTTP client for making requests
#[derive(Clone)]
pub struct HttpClient {
    client: Client,
    config: HttpConfig,
}

impl HttpClient {
    /// Create a new HTTP client with default configuration
    pub fn new() -> Result<Self, HttpError> {
        Self::with_config(HttpConfig::default())
    }

    /// Create a new HTTP client with custom configuration
    pub fn with_config(config: HttpConfig) -> Result<Self, HttpError> {
        let client = Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .redirect(if config.follow_redirects {
                reqwest::redirect::Policy::limited(config.max_redirects)
            } else {
                reqwest::redirect::Policy::none()
            })
            .user_agent(&config.user_agent)
            .build()
            .map_err(|e| HttpError::Request(e.to_string()))?;

        Ok(Self { client, config })
    }

    /// Make a GET request
    pub async fn get(
        &self,
        url: &str,
        headers: Option<HashMap<String, String>>,
    ) -> Result<HttpResponse, HttpError> {
        self.request(Method::GET, url, headers, None).await
    }

    /// Make a POST request with JSON body
    pub async fn post(
        &self,
        url: &str,
        headers: Option<HashMap<String, String>>,
        body: Option<Value>,
    ) -> Result<HttpResponse, HttpError> {
        self.request(Method::POST, url, headers, body).await
    }

    /// Make a PUT request with JSON body
    pub async fn put(
        &self,
        url: &str,
        headers: Option<HashMap<String, String>>,
        body: Option<Value>,
    ) -> Result<HttpResponse, HttpError> {
        self.request(Method::PUT, url, headers, body).await
    }

    /// Make a PATCH request with JSON body
    pub async fn patch(
        &self,
        url: &str,
        headers: Option<HashMap<String, String>>,
        body: Option<Value>,
    ) -> Result<HttpResponse, HttpError> {
        self.request(Method::PATCH, url, headers, body).await
    }

    /// Make a DELETE request
    pub async fn delete(
        &self,
        url: &str,
        headers: Option<HashMap<String, String>>,
    ) -> Result<HttpResponse, HttpError> {
        self.request(Method::DELETE, url, headers, None).await
    }

    /// Make a generic HTTP request
    pub async fn request(
        &self,
        method: Method,
        url: &str,
        headers: Option<HashMap<String, String>>,
        body: Option<Value>,
    ) -> Result<HttpResponse, HttpError> {
        let mut request = self.client.request(method, url);

        // Add default headers from config
        for (key, value) in &self.config.default_headers {
            request = request.header(key.as_str(), value.as_str());
        }

        // Add custom headers (override defaults)
        if let Some(headers) = headers {
            for (key, value) in headers {
                request = request.header(key.as_str(), value.as_str());
            }
        }

        // Add JSON body if provided
        if let Some(body) = body {
            request = request.json(&body);
        }

        // Execute request
        let response = request.send().await?;

        // Convert to HttpResponse
        self.convert_response(response).await
    }

    /// Convert reqwest Response to HttpResponse
    async fn convert_response(&self, response: Response) -> Result<HttpResponse, HttpError> {
        let status = response.status().as_u16();
        let status_text = response
            .status()
            .canonical_reason()
            .unwrap_or("")
            .to_string();
        let ok = response.status().is_success();
        let url = response.url().to_string();

        // Extract headers
        let mut headers = HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(v) = value.to_str() {
                headers.insert(key.to_string(), v.to_string());
            }
        }

        // Get body as text first
        let body_text = response
            .text()
            .await
            .map_err(|e| HttpError::Request(e.to_string()))?;

        // Try to parse as JSON, fall back to string
        let body = serde_json::from_str(&body_text).unwrap_or(Value::String(body_text));

        Ok(HttpResponse {
            status,
            status_text,
            headers,
            body,
            ok,
            url,
        })
    }

    /// Get the current configuration
    pub fn config(&self) -> &HttpConfig {
        &self.config
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new().expect("Failed to create default HTTP client")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = HttpConfig::default();
        assert_eq!(config.timeout_ms, 30000);
        assert!(config.follow_redirects);
        assert_eq!(config.max_redirects, 10);
        assert!(config.user_agent.starts_with("Automodus/"));
    }

    #[test]
    fn test_client_creation() {
        let client = HttpClient::new();
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_with_custom_config() {
        let config = HttpConfig {
            timeout_ms: 5000,
            follow_redirects: false,
            ..Default::default()
        };
        let client = HttpClient::with_config(config.clone());
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.config().timeout_ms, 5000);
        assert!(!client.config().follow_redirects);
    }

    #[test]
    fn test_config_with_default_headers() {
        let mut headers = HashMap::new();
        headers.insert("X-Custom-Header".to_string(), "test-value".to_string());

        let config = HttpConfig {
            default_headers: headers,
            ..Default::default()
        };

        let client = HttpClient::with_config(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_http_error_display() {
        let err = HttpError::Timeout(5000);
        assert_eq!(err.to_string(), "Timeout after 5000ms");

        let err = HttpError::InvalidUrl("bad url".to_string());
        assert_eq!(err.to_string(), "Invalid URL: bad url");

        let err = HttpError::Response {
            status: 404,
            message: "Not Found".to_string(),
        };
        assert_eq!(err.to_string(), "Response error: 404 - Not Found");
    }

    // Live HTTP tests (require network access)
    #[tokio::test]
    async fn test_http_get_request() {
        let client = HttpClient::new().unwrap();

        // Use httpbin.org for testing
        let response = client.get("https://httpbin.org/get", None).await;

        assert!(response.is_ok(), "GET request should succeed");
        let response = response.unwrap();
        assert_eq!(response.status, 200);
        assert!(response.ok);
        assert!(response.body["url"].is_string());
    }

    #[tokio::test]
    async fn test_http_post_request() {
        let client = HttpClient::new().unwrap();

        let body = serde_json::json!({
            "name": "test",
            "value": 42
        });

        let response = client
            .post("https://httpbin.org/post", None, Some(body.clone()))
            .await;

        assert!(response.is_ok(), "POST request should succeed");
        let response = response.unwrap();
        assert_eq!(response.status, 200);
        assert!(response.ok);

        // httpbin echoes back the JSON we sent
        let echoed = &response.body["json"];
        assert_eq!(echoed["name"], "test");
        assert_eq!(echoed["value"], 42);
    }

    #[tokio::test]
    async fn test_http_request_with_headers() {
        let client = HttpClient::new().unwrap();

        let mut headers = HashMap::new();
        headers.insert("X-Test-Header".to_string(), "test-value".to_string());

        let response = client
            .get("https://httpbin.org/headers", Some(headers))
            .await;

        assert!(response.is_ok());
        let response = response.unwrap();

        // httpbin echoes back our headers
        let echoed_headers = &response.body["headers"];
        assert_eq!(echoed_headers["X-Test-Header"], "test-value");
    }

    #[tokio::test]
    async fn test_http_404_response() {
        let client = HttpClient::new().unwrap();

        let response = client.get("https://httpbin.org/status/404", None).await;

        assert!(response.is_ok(), "Should not error on 404");
        let response = response.unwrap();
        assert_eq!(response.status, 404);
        assert!(!response.ok, "404 should not be 'ok'");
    }

    #[tokio::test]
    async fn test_http_invalid_url() {
        let client = HttpClient::new().unwrap();

        let response = client.get("not-a-valid-url", None).await;

        assert!(response.is_err(), "Invalid URL should error");
    }
}
