//! Browser Launch Helpers
//!
//! Common utilities for launching and configuring Chromium browser instances.

use chromiumoxide::browser::{Browser, BrowserConfig};
use futures_util::stream::StreamExt;
use std::path::PathBuf;
use tracing::{debug, info};

/// Options for launching a browser
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Run browser in headless mode
    pub headless: bool,
    /// User data directory for browser profile
    pub user_data_dir: PathBuf,
    /// Additional Chrome arguments (Chromium) / Firefox browser flags
    pub extra_args: Vec<String>,
    /// Chrome/Chromium executable path (ignored when engine is firefox).
    ///
    /// - `None`: auto-detect via [`resolve_chrome_path`] at launch time
    /// - `Some(path)`: must exist; missing path is a hard error (no silent fallback)
    pub chrome_path: Option<PathBuf>,
    /// Firefox executable path (ignored when engine is chromium).
    ///
    /// - `None`: auto-detect via [`resolve_firefox_path`] at launch time
    /// - `Some(path)`: must exist; missing path is a hard error
    pub firefox_path: Option<PathBuf>,
    /// Browser engine backend: `chromium` (CDP) or `firefox` (WebDriver BiDi).
    ///
    /// [`launch_session`] dispatches on this; [`build_browser_config`] only
    /// builds a Chromium [`BrowserConfig`] and fails fast for other engines.
    pub engine: crate::config::BrowserEngine,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            headless: true,
            user_data_dir: std::env::temp_dir().join("automodus-browser"),
            extra_args: vec![],
            chrome_path: None,
            firefox_path: None,
            engine: crate::config::BrowserEngine::default(),
        }
    }
}

impl LaunchOptions {
    /// Create options for running workflow (uses temp profile)
    pub fn for_workflow() -> Self {
        Self {
            headless: true,
            user_data_dir: std::env::temp_dir()
                .join(format!("automodus-workflow-{}", std::process::id())),
            extra_args: vec![],
            chrome_path: None,
            firefox_path: None,
            engine: crate::config::BrowserEngine::default(),
        }
    }

    /// Create options for shell mode (non-headless, temp profile)
    pub fn for_shell() -> Self {
        Self {
            headless: false,
            user_data_dir: std::env::temp_dir()
                .join(format!("automodus-shell-{}", std::process::id())),
            extra_args: vec![],
            chrome_path: None,
            firefox_path: None,
            engine: crate::config::BrowserEngine::default(),
        }
    }

    /// Create options for server mode
    pub fn for_server(headless: bool) -> Self {
        Self {
            headless,
            user_data_dir: std::env::temp_dir().join("automodus-server"),
            extra_args: vec![],
            chrome_path: None,
            firefox_path: None,
            engine: crate::config::BrowserEngine::default(),
        }
    }

    /// Set headless mode
    pub fn headless(mut self, headless: bool) -> Self {
        self.headless = headless;
        self
    }

    /// Set browser engine backend
    pub fn engine(mut self, engine: crate::config::BrowserEngine) -> Self {
        self.engine = engine;
        self
    }

    /// Set user data directory
    pub fn user_data_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_data_dir = path.into();
        self
    }

    /// Add extra Chrome argument
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.extra_args.push(arg.into());
        self
    }

    /// Set Chrome executable path
    pub fn chrome_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.chrome_path = Some(path.into());
        self
    }

    /// Set Firefox executable path
    pub fn firefox_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.firefox_path = Some(path.into());
        self
    }
}

/// Resolve Chrome path from config, env, or common install locations
///
/// Priority: explicit option → `AUTOMODUS_CHROME_PATH` → `CHROME` →
/// `AppConfig.chrome_path` → Playwright/system paths.
///
/// This wrapper reloads AppConfig from the default path (`AUTOMODUS_CONFIG` /
/// `config/app.toml`). When the caller already holds an [`AppConfig`] (e.g.
/// programmatic, test fixture, or custom config path), use
/// [`resolve_chrome_path_with`] to preserve that instance without re-loading.
///
/// Returns `Err` when an **explicitly configured** path is set but missing —
/// never silently substitutes another binary for a bad explicit config.
/// Returns `Ok(None)` only when nothing is configured and no candidate exists
/// (chromiumoxide default discovery then applies).
pub fn resolve_chrome_path() -> Result<Option<PathBuf>, String> {
    resolve_chrome_path_with(None)
}

/// Resolve Chrome path using a caller-provided AppConfig for the config tier
/// (avoids re-loading from disk when the caller already has a config instance).
///
/// Priority unchanged: env → `base` (or default load) → auto-detect.
/// `base = None` is equivalent to [`resolve_chrome_path`].
pub fn resolve_chrome_path_with(
    base: Option<&crate::config::AppConfig>,
) -> Result<Option<PathBuf>, String> {
    fn explicit(path: &str, source: &str) -> Result<Option<PathBuf>, String> {
        if path.is_empty() {
            return Ok(None);
        }
        let p = PathBuf::from(path);
        if p.exists() {
            Ok(Some(p))
        } else {
            Err(format!(
                "{} is set to '{}' but that path does not exist",
                source, path
            ))
        }
    }

    // 1. Explicit env config — missing path is a hard error
    if let Some(p) = explicit(
        &std::env::var("AUTOMODUS_CHROME_PATH").unwrap_or_default(),
        "AUTOMODUS_CHROME_PATH",
    )? {
        return Ok(Some(p));
    }
    // 2. chromiumoxide's CHROME env var — missing path is a hard error
    if let Some(p) = explicit(&std::env::var("CHROME").unwrap_or_default(), "CHROME")? {
        return Ok(Some(p));
    }
    // 3. AppConfig.chrome_path — prefer caller-provided instance.
    // If the held instance has no chrome_path, fall back to disk load so a
    // default-constructed AppConfig doesn't silently ignore config/app.toml.
    // A held instance WITH a value is authoritative (no re-load).
    let config_chrome: Option<PathBuf> = match base {
        Some(c) => c.browser.chrome_path.clone().or_else(|| {
            crate::config::AppConfig::load()
                .ok()
                .and_then(|l| l.browser.chrome_path)
        }),
        None => crate::config::AppConfig::load()
            .ok()
            .and_then(|c| c.browser.chrome_path),
    };
    if let Some(p) = config_chrome {
        if !p.as_os_str().is_empty() {
            if p.exists() {
                return Ok(Some(p));
            }
            return Err(format!(
                "browser.chrome_path is set to '{}' but that path does not exist",
                p.display()
            ));
        }
    }
    // 4. Playwright / common Chromium locations (auto-detect only, per-OS)
    // Playwright revision dirs change with each upgrade — glob for
    // `chromium-*` / `chromium_headless_shell-*` instead of pinning a revision.
    fn playwright_browser() -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        // Playwright cache root differs by OS
        #[cfg(target_os = "macos")]
        let pw_root = home.join("Library/Caches/ms-playwright");
        #[cfg(target_os = "windows")]
        let pw_root = home.join("AppData/Local/ms-playwright");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let pw_root = home.join(".cache/ms-playwright");

        // Binary layout inside a revision dir differs by OS
        #[cfg(target_os = "macos")]
        let (chrome_rel, shell_rel) = ("chrome-mac/Chromium.app/Contents/MacOS/Chromium", "chrome-mac/headless_shell");
        #[cfg(target_os = "windows")]
        let (chrome_rel, shell_rel) = ("chrome-win/chrome.exe", "chrome-win/headless_shell.exe");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let (chrome_rel, shell_rel) = ("chrome-linux64/chrome", "chrome-linux64/headless_shell");

        let mut chromium: Vec<(u64, PathBuf)> = Vec::new();
        let mut shells: Vec<(u64, PathBuf)> = Vec::new();
        for entry in std::fs::read_dir(&pw_root).ok()?.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let rev: u64 = name
                .rsplit('-')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if name.starts_with("chromium_headless_shell-") {
                let p = path.join(shell_rel);
                if p.exists() {
                    shells.push((rev, p));
                }
            } else if name.starts_with("chromium-") {
                let p = path.join(chrome_rel);
                if p.exists() {
                    chromium.push((rev, p));
                }
            }
        }
        // Prefer the newest full Chromium, then newest headless shell
        chromium.sort_by(|a, b| b.0.cmp(&a.0));
        if let Some((_, p)) = chromium.into_iter().next() {
            return Some(p);
        }
        shells.sort_by(|a, b| b.0.cmp(&a.0));
        shells.into_iter().next().map(|(_, p)| p)
    }

    // System browser install locations (per-OS)
    #[cfg(target_os = "macos")]
    let system_candidates: [Option<PathBuf>; 5] = [
        Some(PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")),
        Some(PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium")),
        Some(PathBuf::from("/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary")),
        Some(PathBuf::from("/opt/homebrew/bin/chromium")),
        Some(PathBuf::from("/usr/local/bin/chromium")),
    ];
    #[cfg(target_os = "windows")]
    let system_candidates: [Option<PathBuf>; 4] = [
        Some(PathBuf::from(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        )),
        Some(PathBuf::from(
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        )),
        Some(PathBuf::from(
            r"C:\Program Files\Chromium\Application\chrome.exe",
        )),
        Some(PathBuf::from(
            r"C:\Program Files (x86)\Chromium\Application\chrome.exe",
        )),
    ];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let system_candidates: [Option<PathBuf>; 7] = [
        Some(PathBuf::from("/usr/bin/chromium")),
        Some(PathBuf::from("/usr/bin/chromium-browser")),
        Some(PathBuf::from("/usr/bin/google-chrome")),
        Some(PathBuf::from("/usr/bin/google-chrome-stable")),
        Some(PathBuf::from("/opt/google/chrome/chrome")),
        Some(PathBuf::from("/opt/google/chrome/google-chrome")),
        Some(PathBuf::from("/snap/bin/chromium")),
    ];

    let candidates = std::iter::once(playwright_browser()).chain(system_candidates);
    for c in candidates.flatten() {
        if c.exists() {
            return Ok(Some(c));
        }
    }
    Ok(None)
}

/// Whether proxy-bypass flags should be added.
///
/// Opt-in only: default leaves the system/proxy env alone so corporate
/// HTTP_PROXY / PAC setups keep working. Set `AUTOMODUS_NO_PROXY=1` to force
/// direct connections (useful in CI where gsettings proxy is broken).
fn proxy_bypass_enabled() -> bool {
    matches!(
        std::env::var("AUTOMODUS_NO_PROXY").unwrap_or_default().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Whether `--disable-ipv6` should be added.
///
/// Opt-in only: forcing IPv4 breaks IPv6-only networks. Set
/// `AUTOMODUS_DISABLE_IPV6=1` to prefer IPv4 (useful when dual-stack hangs
/// on broken IPv6 routes).
fn ipv6_disable_enabled() -> bool {
    matches!(
        std::env::var("AUTOMODUS_DISABLE_IPV6").unwrap_or_default().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Default Chrome arguments for stability (static list).
///
/// Returns `Vec<&'static str>` — matches the original public signature
/// (this crate has never shipped `Vec<String>` for this function; an
/// intermediate WIP briefly changed it and was reverted).
///
/// **Migration note:** `--disable-ipv6` is no longer in this default list.
/// It is opt-in via `AUTOMODUS_DISABLE_IPV6=1` (see [`ipv6_disable_enabled`])
/// so IPv6-only networks work out of the box. Dual-stack environments with
/// broken IPv6 that previously relied on the implicit IPv4 preference must
/// set that env var to restore the old behavior.
///
/// Dynamic opt-in flags ([`proxy_bypass_enabled`], [`ipv6_disable_enabled`])
/// are appended separately in [`build_browser_config`].
pub fn default_chrome_args() -> Vec<&'static str> {
    vec![
        "--no-sandbox",
        "--disable-setuid-sandbox",
        "--disable-dev-shm-usage",
        "--disable-web-security",
        "--disable-extensions",
        "--disable-gpu",
        "--no-first-run",
        "--disable-session-crashed-bubble",
        "--disable-infobars",
        // Don't wait on background component updates
        "--disable-component-update",
        "--disable-domain-reliability",
        "--disable-sync",
        "--metrics-recording-only",
        "--no-default-browser-check",
        // NOTE: --disable-ipv6 is opt-in via AUTOMODUS_DISABLE_IPV6 (IPv6-only nets)
    ]
}

/// Dynamic opt-in Chrome args (proxy bypass, IPv6 disable).
/// `pub(crate)` so `BrowserService::initialize` applies the same env flags
/// as the `LaunchOptions` / `build_browser_config` path.
pub(crate) fn dynamic_chrome_args() -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    // Opt-in only — never force direct connections by default
    if proxy_bypass_enabled() {
        args.push("--no-proxy-server".to_string());
        args.push("--proxy-bypass-list=*".to_string());
    }
    // Opt-in only — never break IPv6-only networks by default.
    // Migration: dual-stack users with broken IPv6 who previously relied on the
    // always-on --disable-ipv6 must set AUTOMODUS_DISABLE_IPV6=1. Log a hint
    // when the flag is absent so affected users discover the env var.
    if ipv6_disable_enabled() {
        args.push("--disable-ipv6".to_string());
    } else {
        tracing::debug!(
            "--disable-ipv6 not set (opt-in via AUTOMODUS_DISABLE_IPV6=1). \
             If dual-stack hangs on broken IPv6 routes, set that env var."
        );
    }
    args
}

/// Resolve Firefox path from config, env, or common install locations.
///
/// Priority: explicit `LaunchOptions.firefox_path` (checked by caller) →
/// `AUTOMODUS_FIREFOX_PATH` → `FIREFOX` → system paths.
///
/// Returns `Err` when an **explicitly configured** path is set but missing —
/// never silently substitutes another binary for a bad explicit config.
pub fn resolve_firefox_path(explicit: Option<&PathBuf>) -> Result<PathBuf, String> {
    fn explicit_check(path: &str, source: &str) -> Result<Option<PathBuf>, String> {
        if path.is_empty() {
            return Ok(None);
        }
        let p = PathBuf::from(path);
        if p.exists() {
            Ok(Some(p))
        } else {
            Err(format!(
                "{} is set to '{}' but that path does not exist",
                source, path
            ))
        }
    }

    if let Some(p) = explicit {
        if p.exists() {
            return Ok(p.clone());
        }
        return Err(format!(
            "firefox_path is set to '{}' but that path does not exist",
            p.display()
        ));
    }

    if let Some(p) = explicit_check(
        &std::env::var("AUTOMODUS_FIREFOX_PATH").unwrap_or_default(),
        "AUTOMODUS_FIREFOX_PATH",
    )? {
        return Ok(p);
    }
    if let Some(p) = explicit_check(&std::env::var("FIREFOX").unwrap_or_default(), "FIREFOX")? {
        return Ok(p);
    }

    #[cfg(target_os = "macos")]
    let candidates: [Option<PathBuf>; 3] = [
        Some(PathBuf::from(
            "/Applications/Firefox.app/Contents/MacOS/firefox",
        )),
        Some(PathBuf::from("/usr/local/bin/firefox")),
        Some(PathBuf::from("/opt/homebrew/bin/firefox")),
    ];
    #[cfg(target_os = "windows")]
    let candidates: [Option<PathBuf>; 3] = [
        Some(PathBuf::from(
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
        )),
        Some(PathBuf::from(
            r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe",
        )),
        None,
    ];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let candidates: [Option<PathBuf>; 5] = [
        Some(PathBuf::from("/usr/bin/firefox")),
        Some(PathBuf::from("/usr/local/bin/firefox")),
        Some(PathBuf::from("/snap/bin/firefox")),
        Some(PathBuf::from("/usr/lib/firefox/firefox")),
        Some(PathBuf::from("/opt/firefox/firefox")),
    ];

    for c in candidates.into_iter().flatten() {
        if c.exists() {
            return Ok(c);
        }
    }
    Err(
        "Firefox executable not found; set browser.firefox_path, AUTOMODUS_FIREFOX_PATH, or FIREFOX"
            .into(),
    )
}

/// Build a browser configuration from launch options (Chromium only).
///
/// Firefox does not use this path — see [`launch_session`].
pub fn build_browser_config(options: &LaunchOptions) -> Result<BrowserConfig, String> {
    // Fail fast for engines other than chromium (firefox is launched via
    // launch_session / WebDriver BiDi and must not silently launch Chrome).
    if options.engine != crate::config::BrowserEngine::Chromium {
        return Err(format!(
            "build_browser_config only builds Chromium BrowserConfig (engine='{}'); \
             use launch_session for engine dispatch",
            options.engine
        ));
    }

    // Ensure user data directory exists
    std::fs::create_dir_all(&options.user_data_dir)
        .map_err(|e| format!("Failed to create user data dir: {}", e))?;

    let mut builder = BrowserConfig::builder();

    // Explicit chrome_path must exist — never silently fall through to another binary.
    // When unset, auto-detect (Err only if env/config explicitly point at a missing file).
    let chrome = match &options.chrome_path {
        Some(chrome) => {
            if chrome.exists() {
                Some(chrome.clone())
            } else {
                return Err(format!(
                    "chrome_path is set to '{}' but that path does not exist",
                    chrome.display()
                ));
            }
        }
        None => resolve_chrome_path()?,
    };
    if let Some(ref chrome) = chrome {
        builder = builder.chrome_executable(chrome.as_path());
    }

    // Headless mode
    if !options.headless {
        builder = builder.with_head();
    }

    // Add default stability args
    for arg in default_chrome_args() {
        builder = builder.arg(arg);
    }
    // Dynamic opt-in args (proxy bypass, IPv6 disable)
    for arg in dynamic_chrome_args() {
        builder = builder.arg(arg);
    }

    // User data dir — chromiumoxide's `user_data_dir()` already emits
    // `--user-data-dir=<path>` when building the launch command; do not
    // also pass it via `arg()` (would duplicate the flag).
    builder = builder.user_data_dir(options.user_data_dir.as_path());

    // Add extra args
    for arg in &options.extra_args {
        builder = builder.arg(arg.as_str());
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build browser config: {}", e))
}

/// Launch a browser with the given options
///
/// Returns the browser instance. The handler task is spawned automatically.
pub async fn launch_browser(options: &LaunchOptions) -> Result<Browser, String> {
    let config = build_browser_config(options)?;

    info!(
        headless = options.headless,
        user_data_dir = %options.user_data_dir.display(),
        "Launching browser"
    );

    let (browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| format!("Failed to launch browser: {}", e))?;

    // Spawn handler task to process browser events
    tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if let Err(e) = event {
                debug!("Browser event: {:?}", e);
                if e.to_string().contains("connection closed") {
                    break;
                }
            }
        }
    });

    info!("Browser launched successfully");
    Ok(browser)
}

/// Get or create a page in the browser
pub async fn get_or_create_page(
    browser: &Browser,
    url: Option<&str>,
) -> Result<chromiumoxide::page::Page, String> {
    // Wait a bit for browser to settle
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let pages = browser
        .pages()
        .await
        .map_err(|e| format!("Failed to get pages: {}", e))?;

    if pages.is_empty() {
        let url = url.unwrap_or("about:blank");
        browser
            .new_page(url)
            .await
            .map_err(|e| format!("Failed to create page: {}", e))
    } else {
        Ok(pages.into_iter().next().unwrap())
    }
}

/// Launch a browser session and return a ready [`SessionAdapter`].
///
/// This is the preferred entry point for callers outside `modules/browser` —
/// it encapsulates engine-specific types so the rest of the crate only sees
/// the adapter (which implements [`BrowserHandle`](crate::actions::BrowserHandle)).
///
/// Dispatches on [`LaunchOptions::engine`]:
/// - `chromium` → [`ChromePageAdapter`](super::ChromePageAdapter)
/// - `firefox` → [`FirefoxPageAdapter`](super::FirefoxPageAdapter)
///
/// Does **not** start console/network/crash listeners; call those on the
/// returned adapter (via the trait or inherent methods) if needed.
pub async fn launch_session(options: &LaunchOptions) -> Result<super::SessionAdapter, String> {
    use super::SessionAdapter;

    match options.engine {
        crate::config::BrowserEngine::Chromium => {
            use std::sync::Arc;
            use tokio::sync::Mutex;

            let browser = launch_browser(options).await?;
            let page = get_or_create_page(&browser, None).await?;
            let browser_ref = Arc::new(Mutex::new(Some(browser)));
            Ok(SessionAdapter::Chrome(super::ChromePageAdapter::with_browser(
                page, browser_ref,
            )))
        }
        crate::config::BrowserEngine::Firefox => {
            let config_ff = crate::config::AppConfig::load()
                .ok()
                .and_then(|c| c.browser.firefox_path);
            let explicit = options.firefox_path.as_ref().or(config_ff.as_ref());
            let firefox = resolve_firefox_path(explicit)?;
            let adapter = super::FirefoxPageAdapter::launch(
                firefox,
                options.user_data_dir.clone(),
                options.headless,
                &options.extra_args,
            )
            .await?;
            Ok(SessionAdapter::Firefox(adapter))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize tests that mutate process-global env vars.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_chrome_args_is_static_stable_signature() {
        // Public signature must remain Vec<&'static str> for API stability
        let args: Vec<&'static str> = default_chrome_args();
        assert!(args.iter().any(|a| *a == "--no-sandbox"));
        // Dynamic opt-in flags must NOT be in the static default list
        assert!(!args.iter().any(|a| a.contains("--no-proxy-server")));
        assert!(!args.iter().any(|a| a.contains("--proxy-bypass-list")));
        assert!(!args.iter().any(|a| a.contains("--disable-ipv6")));
    }

    #[test]
    fn dynamic_chrome_args_proxy_and_ipv6_opt_in() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("AUTOMODUS_NO_PROXY");
        std::env::remove_var("AUTOMODUS_DISABLE_IPV6");
        assert!(
            dynamic_chrome_args().is_empty(),
            "no dynamic args by default: {:?}",
            dynamic_chrome_args()
        );

        std::env::set_var("AUTOMODUS_NO_PROXY", "1");
        let args = dynamic_chrome_args();
        assert!(args.iter().any(|a| a == "--no-proxy-server"));
        assert!(args.iter().any(|a| a == "--proxy-bypass-list=*"));
        std::env::remove_var("AUTOMODUS_NO_PROXY");

        std::env::set_var("AUTOMODUS_DISABLE_IPV6", "1");
        let args = dynamic_chrome_args();
        assert!(args.iter().any(|a| a == "--disable-ipv6"));
        std::env::remove_var("AUTOMODUS_DISABLE_IPV6");
    }

    #[test]
    fn missing_explicit_chrome_path_is_hard_error() {
        let opts = LaunchOptions::for_workflow().chrome_path("/definitely/not/a/chrome/binary");
        let err = build_browser_config(&opts).unwrap_err();
        assert!(err.contains("does not exist"), "unexpected err: {}", err);
    }

    #[test]
    fn build_browser_config_rejects_non_chromium_engine() {
        let opts =
            LaunchOptions::for_workflow().engine(crate::config::BrowserEngine::Firefox);
        let err = build_browser_config(&opts).unwrap_err();
        assert!(
            err.contains("only builds Chromium") && err.contains("firefox"),
            "unexpected err: {}",
            err
        );
    }

    #[test]
    fn resolve_firefox_path_rejects_missing_explicit() {
        let err = resolve_firefox_path(Some(&PathBuf::from("/definitely/not/firefox"))).unwrap_err();
        assert!(err.contains("does not exist"), "unexpected err: {}", err);
    }

    #[test]
    fn resolve_firefox_path_rejects_missing_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("AUTOMODUS_FIREFOX_PATH", "/no/such/firefox-xyz");
        let result = resolve_firefox_path(None);
        std::env::remove_var("AUTOMODUS_FIREFOX_PATH");
        let err = result.unwrap_err();
        assert!(err.contains("AUTOMODUS_FIREFOX_PATH"));
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn chromium_engine_builds_config() {
        let opts =
            LaunchOptions::for_workflow().engine(crate::config::BrowserEngine::Chromium);
        // May still fail on missing chrome_path elsewhere, but not on engine
        if let Err(e) = build_browser_config(&opts) {
            assert!(!e.contains("engine"), "must not fail on chromium engine: {}", e);
        }
    }

    #[test]
    fn resolve_chrome_path_rejects_missing_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("AUTOMODUS_CHROME_PATH", "/no/such/chrome-xyz");
        let result = resolve_chrome_path();
        std::env::remove_var("AUTOMODUS_CHROME_PATH");
        let err = result.unwrap_err();
        assert!(err.contains("AUTOMODUS_CHROME_PATH"));
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn resolve_chrome_path_with_preserves_base_config_without_reload() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Clear env so config tier is the only candidate source
        std::env::remove_var("AUTOMODUS_CHROME_PATH");
        std::env::remove_var("CHROME");

        // Build an in-memory AppConfig with a chrome_path that exists
        let mut cfg = crate::config::AppConfig::default();
        // Use a path that exists on this machine (the playwright chrome)
        let real = resolve_chrome_path().ok().flatten();
        if let Some(real) = real {
            cfg.browser.chrome_path = Some(real.clone());
            let got = resolve_chrome_path_with(Some(&cfg)).unwrap();
            assert_eq!(
                got,
                Some(real),
                "must use the provided AppConfig instance, not re-load"
            );
        } else {
            // No chrome on this machine — still must not panic and must not
            // error from a missing default config file
            let _ = resolve_chrome_path_with(Some(&cfg));
        }

        // Missing path in provided config is a hard error (same as reload path)
        let mut bad = crate::config::AppConfig::default();
        bad.browser.chrome_path = Some(PathBuf::from("/definitely/not/chrome"));
        let err = resolve_chrome_path_with(Some(&bad)).unwrap_err();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn resolve_chrome_path_with_empty_base_falls_back_to_disk() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("AUTOMODUS_CHROME_PATH");
        std::env::remove_var("CHROME");

        // A default-constructed AppConfig (chrome_path=None) must NOT prevent
        // discovering browser.chrome_path from config/app.toml on disk.
        // Both paths (held-empty and None) must agree on the config tier.
        let empty = crate::config::AppConfig::default();
        assert!(empty.browser.chrome_path.is_none());

        let via_empty_base = resolve_chrome_path_with(Some(&empty)).unwrap();
        let via_none = resolve_chrome_path_with(None).unwrap();
        assert_eq!(
            via_empty_base, via_none,
            "empty held config must fall back to disk load, matching resolve_chrome_path()"
        );
    }
}
