//! HTTP Module
//!
//! Provides HTTP client actions for making API requests within workflows.
//!
//! ## Actions
//!
//! - `http.get` - Make GET request
//! - `http.post` - Make POST request with JSON body
//! - `http.put` - Make PUT request with JSON body
//! - `http.patch` - Make PATCH request with JSON body
//! - `http.delete` - Make DELETE request
//! - `http.request` - Generic request (any method)
//!
//! ## Example
//!
//! ```yaml
//! steps:
//!   - action: http.get
//!     url: "https://api.example.com/users"
//!     headers:
//!       Authorization: "Bearer {{params.token}}"
//!     store_as: users
//!
//!   - action: http.post
//!     url: "https://api.example.com/data"
//!     body:
//!       name: "{{params.name}}"
//!       value: 42
//!     store_as: result
//! ```

pub mod actions;
pub mod client;

pub use actions::{
    HttpDeleteAction, HttpGetAction, HttpPatchAction, HttpPostAction, HttpPutAction,
    HttpRequestAction,
};
pub use client::{HttpClient, HttpConfig};
