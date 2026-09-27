//! Wait actions: wait_for, sleep
//!
//! wait_for supports:
//!   - selector: CSS selector to wait for
//!   - state: "visible" (default), "hidden", "present", "absent"
//!   - text: Wait for text to appear/disappear
//!   - url: Wait for URL to match condition
//!   - timeout: Timeout in ms (default: 30000)

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

use crate::actions::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};

/// Element state to wait for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitState {
    /// Element exists and is visible (default)
    Visible,
    /// Element is hidden or doesn't exist
    Hidden,
    /// Element exists in DOM (may be hidden)
    Present,
    /// Element does not exist in DOM
    Absent,
}

impl WaitState {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "hidden" | "invisible" => Self::Hidden,
            "present" | "exists" => Self::Present,
            "absent" | "removed" | "gone" => Self::Absent,
            _ => Self::Visible,
        }
    }
}

/// Whether an eval error is a known transient CDP failure during navigation.
///
/// Classified on the typed `ActionError::BrowserError` payload produced by
/// `ChromePageAdapter::eval` (which embeds the chromiumoxide error text).
///
/// Deliberately **excludes** `Target closed` / `Session closed`: those mean the
/// browser/tab is gone and retrying can never succeed — fail fast instead of
/// burning the full timeout retrying a dead session.
///
/// Matching is case-insensitive so variants like `Frame detached` /
/// `Execution Context was destroyed` still classify correctly.
fn is_transient_nav_eval_error(e: &ActionError) -> bool {
    let ActionError::BrowserError(msg) = e else {
        return false;
    };
    let msg = msg.to_ascii_lowercase();
    // Match on stable CDP protocol error strings (not localized UI text).
    // Keep this list tight so genuine script errors still fail fast.
    // Patterns stored lowercase; comparison lowercases the message.
    const TRANSIENT: &[&str] = &[
        "cannot find context",
        "cannot find context with specified id",
        "execution context was destroyed",
        "execution context is not available",
        "context was destroyed",
        "frame detached",
        "inspected target navigated or closed",
    ];
    TRANSIENT.iter().any(|p| msg.contains(p))
}

/// Whether an eval error means the browser/tab session is permanently gone.
fn is_fatal_session_error(e: &ActionError) -> bool {
    let ActionError::BrowserError(msg) = e else {
        return false;
    };
    let msg = msg.to_ascii_lowercase();
    const FATAL: &[&str] = &["target closed", "session closed"];
    FATAL.iter().any(|p| msg.contains(p))
}

/// Wait for element with configurable state
pub struct WaitForAction;

#[async_trait]
impl Action for WaitForAction {
    fn name(&self) -> &'static str {
        "wait_for"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let timeout = params
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(30000);

        // Get state parameter (visible, hidden, present, absent)
        let state = params
            .get("state")
            .and_then(|v| v.as_str())
            .map(WaitState::from_str)
            .unwrap_or(WaitState::Visible);

        // Element selector
        if let Some(selector) = params.get("selector").and_then(|v| v.as_str()) {
            match state {
                WaitState::Visible | WaitState::Present => {
                    browser.wait_for(selector, timeout).await?;
                    return Ok(ActionOutput::with_data(json!({
                        "found": selector,
                        "state": "visible"
                    })));
                }
                WaitState::Hidden | WaitState::Absent => {
                    browser.wait_for_hidden(selector, timeout).await?;
                    return Ok(ActionOutput::with_data(json!({
                        "selector": selector,
                        "state": "hidden"
                    })));
                }
            }
        }

        // Wait for URL condition
        if let Some(url) = params.get("url").and_then(|v| v.as_str()) {
            browser.wait_for_url(url, timeout).await?;
            let current = browser.current_url().await?;
            return Ok(ActionOutput::with_data(json!({
                "url": current
            })));
        }

        // Wait for text to appear/disappear
        if let Some(text) = params.get("text").and_then(|v| v.as_str()) {
            // Use JavaScript to find text since XPath can be finicky.
            // Visibility semantics: when `body` exists, prefer `innerText`
            // (visible text). Fall back to `textContent` ONLY when `innerText`
            // is null/undefined (XML/SVG edge docs) — not when it is
            // legitimately empty (`'' ?? x` returns `''`), so empty visible
            // text never false-positives via hidden/head content.
            // Embed the needle as a JSON string literal so newlines, U+2028/29,
            // quotes, and other control chars are safely escaped for JS.
            let needle = serde_json::to_string(text)
                .map_err(|e| ActionError::Internal(format!("Invalid wait text: {}", e)))?;
            let script = format!(
                r#"(document.body ? (document.body.innerText ?? document.documentElement?.textContent ?? '') : (document.documentElement?.textContent ?? '')).includes({needle})"#
            );

            let start = std::time::Instant::now();
            let duration = Duration::from_millis(timeout);
            let mut transient_retries: u32 = 0;

            loop {
                // Navigation (e.g. form submit) can destroy the JS context mid-poll.
                // A transient eval failure means "unknown" — neither found nor absent.
                // Only a successful eval decides the wait; unknown never counts as
                // success (would false-positive disappear waits) or definitive
                // failure. The loop is already time-bounded by `timeout`; do NOT
                // hard-fail on a retry count (a slow navigation with rapid polls
                // must still get the full timeout budget).
                let eval_result: Option<bool> = match browser.eval(&script).await {
                    Ok(result) => {
                        transient_retries = 0;
                        Some(result.as_bool().unwrap_or(false))
                    }
                    Err(e) => {
                        if is_fatal_session_error(&e) {
                            return Err(e);
                        }
                        if is_transient_nav_eval_error(&e) {
                            transient_retries += 1;
                            tracing::debug!(
                                retry = transient_retries,
                                error = %e,
                                "wait_for text: transient eval error during navigation, retrying"
                            );
                            None // unknown — do not decide success/failure this tick
                        } else {
                            return Err(e);
                        }
                    }
                };

                // Only act on a real observation; skip unknown ticks entirely.
                if let Some(found) = eval_result {
                    match state {
                        WaitState::Visible | WaitState::Present => {
                            if found {
                                return Ok(ActionOutput::with_data(json!({
                                    "text_found": text,
                                    "state": "visible"
                                })));
                            }
                        }
                        WaitState::Hidden | WaitState::Absent => {
                            if !found {
                                return Ok(ActionOutput::with_data(json!({
                                    "text_gone": text,
                                    "state": "hidden"
                                })));
                            }
                        }
                    }
                }

                if start.elapsed() >= duration {
                    let expected = match state {
                        WaitState::Visible | WaitState::Present => "appear",
                        WaitState::Hidden | WaitState::Absent => "disappear",
                    };
                    return Err(ActionError::Timeout(format!(
                        "Text '{}' did not {} within {}ms",
                        text, expected, timeout
                    )));
                }

                sleep(Duration::from_millis(100)).await;
            }
        }

        Err(ActionError::MissingParameter(
            "selector, url, or text".to_string(),
        ))
    }
}

/// Sleep for specified duration
pub struct SleepAction;

#[async_trait]
impl Action for SleepAction {
    fn name(&self) -> &'static str {
        "sleep"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        _browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let ms = params
            .get("ms")
            .or_else(|| params.get("duration"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1000);

        sleep(Duration::from_millis(ms)).await;

        Ok(ActionOutput::with_data(json!({
            "slept_ms": ms
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_nav_errors_are_classified() {
        let transient = [
            "Browser error: Eval failed: Error -32000: Cannot find context with specified id",
            "Browser error: Eval failed: Execution context was destroyed.",
            "Browser error: Eval failed: frame detached.",
            // Case variants that CDP/chromiumoxide may emit
            "Browser error: Eval failed: Frame detached.",
            "Browser error: Eval failed: Execution Context was destroyed.",
            "Browser error: Eval failed: Context Was Destroyed",
        ];
        for msg in transient {
            let e = ActionError::BrowserError(msg.to_string());
            assert!(
                is_transient_nav_eval_error(&e),
                "should be transient: {}",
                msg
            );
            assert!(!is_fatal_session_error(&e), "should not be fatal: {}", msg);
        }

        // Dead browser/session — fail fast, never retry into a timeout
        let fatal = [
            "Browser error: Eval failed: Target closed",
            "Browser error: Eval failed: Session closed",
            "Browser error: Eval failed: target closed",
            "Browser error: Eval failed: SESSION CLOSED",
        ];
        for msg in fatal {
            let e = ActionError::BrowserError(msg.to_string());
            assert!(!is_transient_nav_eval_error(&e), "must not retry: {}", msg);
            assert!(is_fatal_session_error(&e), "should be fatal: {}", msg);
        }

        let hard = [
            "Browser error: Eval failed: Uncaught ReferenceError: foo is not defined",
            "Browser error: Eval failed: SyntaxError: Unexpected token",
            "Internal error: something else",
            "Timeout: waited too long",
        ];
        for msg in hard {
            let e = match msg.starts_with("Internal") {
                true => ActionError::Internal(msg.to_string()),
                false if msg.starts_with("Timeout") => ActionError::Timeout(msg.to_string()),
                false => ActionError::BrowserError(msg.to_string()),
            };
            assert!(!is_transient_nav_eval_error(&e), "should be hard: {}", msg);
            assert!(!is_fatal_session_error(&e), "should not be fatal: {}", msg);
        }
    }

    #[test]
    fn wait_text_needle_is_json_escaped() {
        // Newlines and quotes must survive into a JS string literal
        let text = "line1\nline2 \"quoted\" 'single' \\ and \u{2028}sep";
        let needle = serde_json::to_string(text).unwrap();
        let script = format!("('' ).includes({needle})");
        // Round-trip: parse the embedded literal back
        let start = script.find("includes(").unwrap() + "includes(".len();
        let end = script.rfind(')').unwrap();
        let lit = &script[start..end];
        let parsed: String = serde_json::from_str(lit).expect("valid JSON string literal");
        assert_eq!(parsed, text);
    }

    /// Body-exists path must use only innerText (not OR with textContent),
    /// so empty innerText cannot false-positive Visible waits via hidden/head text.
    /// Nullish-coalesce so null/undefined innerText becomes '' (no TypeError).
    #[test]
    fn wait_text_script_uses_strict_body_inner_text() {
        let needle = serde_json::to_string("Hello").unwrap();
        let script = format!(
            r#"(document.body ? (document.body.innerText ?? document.documentElement?.textContent ?? '') : (document.documentElement?.textContent ?? '')).includes({needle})"#
        );
        // Must not use the old `body.innerText || documentElement.textContent` OR-fallback
        // (`||` would treat legitimately-empty innerText as falsy and OR into hidden text)
        assert!(
            !script.contains("body.innerText ||"),
            "must not OR-fallback to textContent when body exists: {}",
            script
        );
        // Must nullish-coalesce (??) so null/undefined innerText falls back,
        // but empty string '' does NOT (avoids hidden-text false positives)
        assert!(
            script.contains("document.body.innerText ?? document.documentElement?.textContent"),
            "must nullish-coalesce innerText then fall back to textContent: {}",
            script
        );
        assert!(
            script.contains("document.body ? (document.body.innerText"),
            "must use ternary on body existence: {}",
            script
        );
    }

    /// Disappear waits must not treat a transient unknown as "text gone".
    /// This mirrors the eval_result semantics: None = unknown, never decide.
    #[test]
    fn unknown_eval_result_does_not_satisfy_disappear_wait() {
        // Simulate the decision logic: only Some(false) may satisfy Hidden/Absent.
        let state = WaitState::Hidden;
        let cases: [(Option<bool>, bool); 3] = [
            (None, false),       // unknown — must not succeed
            (Some(true), false), // text still present — must not succeed
            (Some(false), true), // text confirmed gone — success
        ];
        for (eval_result, should_succeed) in cases {
            let mut succeeded = false;
            if let Some(found) = eval_result {
                if matches!(state, WaitState::Hidden | WaitState::Absent) && !found {
                    succeeded = true;
                }
            }
            assert_eq!(
                succeeded, should_succeed,
                "eval_result={:?} should_succeed={}",
                eval_result, should_succeed
            );
        }
    }
}
