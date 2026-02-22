//! Automodus - Programmable Workflow Automation Platform
//!
//! CLI for running YAML-based automation workflows.
//!
//! ## Usage
//!
//! ```bash
//! # Run a workflow
//! automodus run workflows/example.yaml
//!
//! # Start the API server
//! automodus serve
//!
//! # Validate workflows
//! automodus validate workflows/
//! ```

use futures_util::stream::StreamExt;
use futures_util::FutureExt;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::debug;

use automodus::{
    actions::BrowserHandle,
    api,
    core::WorkflowEngine,
    daemon::{Daemon, DaemonConfig, DaemonStatus},
    modules::ChromePageAdapter,
    utils::{logging, yaml_to_json},
    workflow::{
        schema::{CaptureMode, DebugConfig, DebugProfile, LogLevel},
        WorkflowLoader, WorkflowParser,
    },
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Read debug configuration from environment variables.
///
/// Supported variables:
/// - `AUTOMODUS_DEBUG` - Enable debug mode (1, true, yes, on)
/// - `AUTOMODUS_DEBUG_LEVEL` - Set log level (info, debug, trace)
/// - `AUTOMODUS_DEBUG_PROFILE` - Use a preset (minimal, verbose, ci, demo)
/// - `AUTOMODUS_DEBUG_DELAY` - Step delay in milliseconds
/// - `AUTOMODUS_DEBUG_CAPTURE` - Screenshot mode (none, failure, before, after, all)
fn debug_config_from_env() -> DebugConfig {
    let mut config = DebugConfig::default();

    // Check for profile first (base config)
    if let Ok(profile) = std::env::var("AUTOMODUS_DEBUG_PROFILE") {
        let profile_config = match profile.to_lowercase().as_str() {
            "minimal" => DebugProfile::Minimal.to_config(),
            "verbose" => DebugProfile::Verbose.to_config(),
            "ci" => DebugProfile::Ci.to_config(),
            "demo" => DebugProfile::Demo.to_config(),
            _ => {
                eprintln!(
                    "Warning: Invalid AUTOMODUS_DEBUG_PROFILE '{}', ignoring",
                    profile
                );
                DebugConfig::default()
            }
        };
        config = profile_config;
    }

    // Enable/disable (overrides profile)
    if let Ok(val) = std::env::var("AUTOMODUS_DEBUG") {
        let enabled = matches!(val.to_lowercase().as_str(), "1" | "true" | "yes" | "on");
        config.enabled = Some(enabled);
    }

    // Log level (overrides profile)
    if let Ok(level) = std::env::var("AUTOMODUS_DEBUG_LEVEL") {
        config.level = Some(match level.to_lowercase().as_str() {
            "info" => LogLevel::Info,
            "debug" => LogLevel::Debug,
            "trace" => LogLevel::Trace,
            _ => {
                eprintln!(
                    "Warning: Invalid AUTOMODUS_DEBUG_LEVEL '{}', using info",
                    level
                );
                LogLevel::Info
            }
        });
    }

    // Step delay (overrides profile)
    if let Ok(delay) = std::env::var("AUTOMODUS_DEBUG_DELAY") {
        if let Ok(ms) = delay.parse::<u64>() {
            config.delay = Some(ms);
        } else {
            eprintln!(
                "Warning: Invalid AUTOMODUS_DEBUG_DELAY '{}', ignoring",
                delay
            );
        }
    }

    // Capture mode (overrides profile)
    if let Ok(mode) = std::env::var("AUTOMODUS_DEBUG_CAPTURE") {
        config.capture = Some(match mode.to_lowercase().as_str() {
            "none" => CaptureMode::None,
            "failure" => CaptureMode::Failure,
            "before" => CaptureMode::Before,
            "after" => CaptureMode::After,
            "all" => CaptureMode::All,
            _ => {
                eprintln!(
                    "Warning: Invalid AUTOMODUS_DEBUG_CAPTURE '{}', using none",
                    mode
                );
                CaptureMode::None
            }
        });
    }

    config
}

fn print_banner() {
    println!(
        r#"
    _         _                            _           
   / \  _   _| |_ ___  _ __ ___   ___   __| |_   _ ___ 
  / _ \| | | | __/ _ \| '_ ` _ \ / _ \ / _` | | | / __|
 / ___ \ |_| | || (_) | | | | | | (_) | (_| | |_| \__ \
/_/   \_\__,_|\__\___/|_| |_| |_|\___/ \__,_|\__,_|___/
                                                       v{}
"#,
        VERSION
    );
}

#[derive(Debug)]
enum Command {
    /// Run a specific workflow file
    Run {
        path: PathBuf,
        keep_open: bool,
        /// CLI debug configuration (overrides workflow config)
        debug: DebugConfig,
    },
    /// Start the API server
    Serve,
    /// Validate workflow files
    Validate { path: PathBuf },
    /// List all loaded workflows
    List,
    /// Interactive shell mode - keeps browser running
    Shell,
    /// Daemon operations
    Daemon { operation: DaemonOp },
    /// Show help
    Help,
}

#[derive(Debug)]
enum DaemonOp {
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Show daemon status
    Status,
    /// Restart the daemon
    Restart,
    /// View daemon logs
    Logs { follow: bool, lines: usize },
}

fn parse_args() -> Command {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        return Command::Help;
    }

    match args[1].as_str() {
        "run" => {
            if args.len() < 3 {
                eprintln!("Usage: automodus run <workflow.yaml> [OPTIONS]");
                eprintln!("Try 'automodus help' for more information.");
                std::process::exit(1);
            }
            let keep_open = args.iter().any(|a| a == "--keep-open" || a == "-k");

            // Parse debug flags
            let mut debug = DebugConfig::default();

            for arg in &args[3..] {
                if arg == "--debug" || arg == "-d" {
                    debug.enabled = Some(true);
                } else if let Some(level) = arg.strip_prefix("--debug=") {
                    debug.enabled = Some(true);
                    debug.level = Some(match level.to_lowercase().as_str() {
                        "info" => LogLevel::Info,
                        "debug" => LogLevel::Debug,
                        "trace" => LogLevel::Trace,
                        _ => {
                            eprintln!("Invalid debug level: {}. Use: info, debug, trace", level);
                            std::process::exit(1);
                        }
                    });
                } else if let Some(delay) = arg.strip_prefix("--delay=") {
                    debug.enabled = Some(true);
                    match delay.parse::<u64>() {
                        Ok(ms) => debug.delay = Some(ms),
                        Err(_) => {
                            eprintln!("Invalid delay value: {}. Expected milliseconds.", delay);
                            std::process::exit(1);
                        }
                    }
                } else if let Some(mode) = arg.strip_prefix("--capture=") {
                    debug.enabled = Some(true);
                    debug.capture = Some(match mode.to_lowercase().as_str() {
                        "none" => CaptureMode::None,
                        "failure" => CaptureMode::Failure,
                        "before" => CaptureMode::Before,
                        "after" => CaptureMode::After,
                        "all" => CaptureMode::All,
                        _ => {
                            eprintln!(
                                "Invalid capture mode: {}. Use: none, failure, before, after, all",
                                mode
                            );
                            std::process::exit(1);
                        }
                    });
                } else if let Some(profile) = arg.strip_prefix("--profile=") {
                    debug.enabled = Some(true);
                    let profile_config = match profile.to_lowercase().as_str() {
                        "minimal" => DebugProfile::Minimal.to_config(),
                        "verbose" => DebugProfile::Verbose.to_config(),
                        "ci" => DebugProfile::Ci.to_config(),
                        "demo" => DebugProfile::Demo.to_config(),
                        _ => {
                            eprintln!(
                                "Invalid profile: {}. Use: minimal, verbose, ci, demo",
                                profile
                            );
                            std::process::exit(1);
                        }
                    };
                    // Profile is applied as base, then other CLI flags override
                    debug = profile_config.merge(&debug);
                } else if arg == "--highlight" {
                    debug.enabled = Some(true);
                    debug.highlight = Some(true);
                } else if arg == "--pause" {
                    debug.enabled = Some(true);
                    debug.pause = Some(true);
                } else if arg == "--console" {
                    debug.enabled = Some(true);
                    debug.console = Some(true);
                } else if arg == "--network" {
                    debug.enabled = Some(true);
                    debug.network = Some(true);
                }
            }

            Command::Run {
                path: PathBuf::from(&args[2]),
                keep_open,
                debug,
            }
        }
        "shell" => Command::Shell,
        "serve" => Command::Serve,
        "daemon" => {
            if args.len() < 3 {
                eprintln!("Usage: automodus daemon <start|stop|status|restart|logs>");
                eprintln!("Try 'automodus help' for more information.");
                std::process::exit(1);
            }
            let operation = match args[2].as_str() {
                "start" => DaemonOp::Start,
                "stop" => DaemonOp::Stop,
                "status" => DaemonOp::Status,
                "restart" => DaemonOp::Restart,
                "logs" => {
                    let follow = args.iter().any(|a| a == "--follow" || a == "-f");
                    let mut lines: usize = 50;
                    for arg in &args[3..] {
                        if let Some(n) = arg.strip_prefix("--lines=") {
                            lines = n.parse().unwrap_or(50);
                        } else if let Some(n) = arg.strip_prefix("-n") {
                            lines = n.parse().unwrap_or(50);
                        }
                    }
                    DaemonOp::Logs { follow, lines }
                }
                _ => {
                    eprintln!("Unknown daemon operation: {}", args[2]);
                    eprintln!("Use: start, stop, status, restart, logs");
                    std::process::exit(1);
                }
            };
            Command::Daemon { operation }
        }
        "validate" => {
            let path = if args.len() >= 3 {
                PathBuf::from(&args[2])
            } else {
                PathBuf::from("workflows")
            };
            Command::Validate { path }
        }
        "list" => Command::List,
        "--help" | "-h" | "help" => Command::Help,
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            Command::Help
        }
    }
}

fn print_help() {
    println!(
        r#"
Automodus - Programmable Workflow Automation Platform

USAGE:
    automodus <COMMAND> [OPTIONS]

COMMANDS:
    run <workflow.yaml>     Run a specific workflow file
        -k, --keep-open     Keep browser open after workflow completes
        -d, --debug         Enable debug mode
        --debug=<level>     Set debug level (info, debug, trace)
        --delay=<ms>        Add delay between steps (milliseconds)
        --capture=<mode>    Screenshot capture (none, failure, before, after, all)
        --profile=<name>    Use debug preset (minimal, verbose, ci, demo)
        --highlight         Highlight elements before interaction
        --pause             Pause before each step (requires confirmation)
        --console           Log browser console messages
        --network           Log network requests

    daemon <operation>  Daemon process management
        start           Start the daemon (background process)
        stop            Stop the running daemon
        status          Check daemon status
        restart         Restart the daemon
        logs            View daemon logs
            -f, --follow    Follow log output
            --lines=<n>     Number of lines to show (default: 50)

    shell               Interactive shell mode (keeps browser running)
    serve               [DEPRECATED] Start HTTP server in foreground
                        (Use 'daemon start' for background)
    validate [path]     Validate workflow files (default: workflows/)
    list                List all loaded workflows
    help                Show this help message

DEBUG PROFILES:
    minimal     Basic logging, no delays or captures
    verbose     Full logging with highlights and networking
    ci          Capture on failure for CI pipelines
    demo        Slow execution with delays and highlights

EXAMPLES:
    # Run a specific workflow
    automodus run workflows/example.yaml

    # Run workflow with debug mode
    automodus run workflows/login.yaml --debug

    # Run with verbose profile and custom delay
    automodus run workflows/test.yaml --profile=verbose --delay=1000

    # Run with screenshots on failure (for CI)
    automodus run workflows/test.yaml --capture=failure

    # Run workflow and keep browser open
    automodus run workflows/login.yaml --keep-open

    # Start the daemon
    automodus daemon start

    # Check daemon status
    automodus daemon status

    # View daemon logs (follow mode)
    automodus daemon logs -f

    # Start interactive shell for testing
    automodus shell

    # Validate all workflows in a directory
    automodus validate workflows/

ENVIRONMENT:
    AUTOMODUS_CONFIG          Path to config file (default: config/app.toml)
    AUTOMODUS_WORKFLOWS       Path to workflows directory (default: workflows/)
    RUST_LOG                  Logging level (default: info)

    Debug environment variables (lowest precedence):
    AUTOMODUS_DEBUG           Enable debug mode (1, true, yes, on)
    AUTOMODUS_DEBUG_LEVEL     Log level (info, debug, trace)
    AUTOMODUS_DEBUG_PROFILE   Use preset (minimal, verbose, ci, demo)
    AUTOMODUS_DEBUG_DELAY     Step delay in milliseconds
    AUTOMODUS_DEBUG_CAPTURE   Screenshot mode (none, failure, before, after, all)

For more information, visit: https://github.com/devstroop/automodus
"#
    );
}

/// Parse shell parameters handling quoted values with spaces
/// e.g., `phone=1234 caption="Hi There" file=/path` -> ["phone=1234", "caption=\"Hi There\"", "file=/path"]
fn parse_shell_params(input: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';

    for ch in input.chars() {
        match ch {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
                current.push(ch);
            }
            c if c == quote_char && in_quotes => {
                in_quotes = false;
                current.push(ch);
            }
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    result.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup logging
    logging::init_logging().unwrap_or_else(|e| {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    });

    let command = parse_args();

    match command {
        Command::Help => {
            print_banner();
            print_help();
        }
        Command::Run {
            path,
            keep_open,
            debug,
        } => {
            print_banner();
            run_workflow(&path, keep_open, debug).await?;
        }
        Command::Shell => {
            print_banner();
            run_shell().await?;
        }
        Command::Daemon { operation } => {
            handle_daemon_command(operation).await?;
        }
        Command::Serve => {
            print_banner();
            println!("⚠️  DEPRECATED: 'automodus serve' is deprecated.");
            println!("   Use 'automodus daemon start' instead for background operation,");
            println!("   or the daemon will now start in foreground mode.\n");
            
            // Start daemon in foreground with HTTP enabled
            let config = DaemonConfig {
                enable_http: true,
                ..DaemonConfig::default()
            };
            
            let daemon = Daemon::new(config.clone());
            
            // Check if daemon is already running
            if daemon.is_running() {
                println!("❌ Daemon is already running");
                if let DaemonStatus::Running { pid } = daemon.status() {
                    println!("   PID: {}", pid);
                }
                println!("\nUse 'automodus daemon stop' to stop it first.");
                std::process::exit(1);
            }
            
            println!("🚀 Starting server (foreground mode)...");
            println!("   HTTP: http://{}:{}", config.http_host, config.http_port);
            println!("   Press Ctrl+C to stop\n");
            
            // Run the API server directly (foreground)
            api::run_server().await?;
        }
        Command::Validate { path } => {
            validate_workflows(&path)?;
        }
        Command::List => {
            list_workflows().await?;
        }
    }

    Ok(())
}

/// Handle daemon commands
async fn handle_daemon_command(op: DaemonOp) -> Result<(), Box<dyn std::error::Error>> {
    let config = DaemonConfig::default();

    match op {
        DaemonOp::Start => {
            println!("🚀 Starting automodus daemon...");

            // Check if already running
            let daemon = Daemon::new(config.clone());
            if daemon.is_running() {
                println!("❌ Daemon is already running");
                if let DaemonStatus::Running { pid } = daemon.status() {
                    println!("   PID: {}", pid);
                }
                std::process::exit(1);
            }

            // Fork to background (Unix only)
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                use std::process::Command;

                // Create the data directory
                if let Some(parent) = config.socket_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                // Start daemon in background
                let exe = std::env::current_exe()?;
                let mut cmd = Command::new(exe);
                cmd.arg("daemon").arg("__run__");

                // Detach from terminal
                unsafe {
                    cmd.pre_exec(|| {
                        // Create new session
                        libc::setsid();
                        Ok(())
                    });
                }

                let child = cmd.spawn()?;
                println!("✓ Daemon started (PID: {})", child.id());
                println!("  Socket: {}", config.socket_path.display());
                println!("  Log: {}", config.log_file.display());
            }

            #[cfg(not(unix))]
            {
                println!("❌ Daemon mode is only supported on Unix systems");
                std::process::exit(1);
            }
        }

        DaemonOp::Stop => {
            let daemon = Daemon::new(config.clone());

            if !daemon.is_running() {
                println!("ℹ️  Daemon is not running");
                return Ok(());
            }

            // Read PID and send SIGTERM
            if let Ok(pid_str) = std::fs::read_to_string(&config.pid_file) {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    println!("Stopping daemon (PID: {})...", pid);

                    #[cfg(unix)]
                    {
                        unsafe {
                            libc::kill(pid, libc::SIGTERM);
                        }
                    }

                    // Wait for shutdown
                    for _ in 0..10 {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        if !daemon.is_running() {
                            break;
                        }
                    }

                    if daemon.is_running() {
                        println!("⚠️  Daemon did not stop gracefully, forcing...");
                        #[cfg(unix)]
                        unsafe {
                            libc::kill(pid, libc::SIGKILL);
                        }
                    }

                    println!("✓ Daemon stopped");
                }
            }

            // Cleanup stale files
            let _ = std::fs::remove_file(&config.pid_file);
            let _ = std::fs::remove_file(&config.socket_path);
        }

        DaemonOp::Status => {
            let daemon = Daemon::new(config.clone());
            let status = daemon.status();

            match status {
                DaemonStatus::Running { pid } => {
                    println!("✓ Daemon is running");
                    println!("  PID: {}", pid);
                    println!("  Socket: {}", config.socket_path.display());
                }
                DaemonStatus::Stopped => {
                    println!("○ Daemon is stopped");
                }
            }
        }

        DaemonOp::Restart => {
            println!("🔄 Restarting daemon...");

            // Stop if running (inline to avoid recursion)
            let daemon = Daemon::new(config.clone());
            if daemon.is_running() {
                if let Ok(pid_str) = std::fs::read_to_string(&config.pid_file) {
                    if let Ok(pid) = pid_str.trim().parse::<i32>() {
                        println!("Stopping daemon (PID: {})...", pid);

                        #[cfg(unix)]
                        {
                            unsafe {
                                libc::kill(pid, libc::SIGTERM);
                            }
                        }

                        // Wait for shutdown
                        for _ in 0..10 {
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            if !daemon.is_running() {
                                break;
                            }
                        }

                        if daemon.is_running() {
                            #[cfg(unix)]
                            unsafe {
                                libc::kill(pid, libc::SIGKILL);
                            }
                        }
                    }
                }
                let _ = std::fs::remove_file(&config.pid_file);
                let _ = std::fs::remove_file(&config.socket_path);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }

            // Start (inline to avoid recursion)
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                use std::process::Command;

                if let Some(parent) = config.socket_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                let exe = std::env::current_exe()?;
                let mut cmd = Command::new(exe);
                cmd.arg("daemon").arg("__run__");

                unsafe {
                    cmd.pre_exec(|| {
                        libc::setsid();
                        Ok(())
                    });
                }

                let child = cmd.spawn()?;
                println!("✓ Daemon restarted (PID: {})", child.id());
            }

            #[cfg(not(unix))]
            {
                println!("❌ Daemon mode is only supported on Unix systems");
                std::process::exit(1);
            }
        }

        DaemonOp::Logs { follow, lines } => {
            if !config.log_file.exists() {
                println!("No log file found at: {}", config.log_file.display());
                return Ok(());
            }

            if follow {
                // Tail -f mode
                println!("Following daemon logs (Ctrl+C to stop)...\n");

                use std::io::{BufRead, BufReader, Seek, SeekFrom};

                let file = std::fs::File::open(&config.log_file)?;
                let mut reader = BufReader::new(file);

                // Seek to end first
                reader.seek(SeekFrom::End(0))?;

                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            // No new data, wait
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                        Ok(_) => {
                            print!("{}", line);
                        }
                        Err(e) => {
                            eprintln!("Error reading log: {}", e);
                            break;
                        }
                    }

                    // Check for Ctrl+C
                    if tokio::signal::ctrl_c().now_or_never().is_some() {
                        break;
                    }
                }
            } else {
                // Show last N lines
                let content = std::fs::read_to_string(&config.log_file)?;
                let all_lines: Vec<&str> = content.lines().collect();
                let start = all_lines.len().saturating_sub(lines);

                for line in &all_lines[start..] {
                    println!("{}", line);
                }
            }
        }
    }

    Ok(())
}

async fn run_workflow(
    path: &std::path::Path,
    keep_open: bool,
    cli_debug: DebugConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Loading workflow from: {}", path.display());

    // Parse workflow
    let content = std::fs::read_to_string(path)?;
    let workflow = WorkflowParser::parse(&content)?;

    // Merge debug configs: env (lowest) → workflow → CLI (highest)
    let env_debug = debug_config_from_env();
    let resolved_debug = env_debug
        .merge(&workflow.debug)
        .merge(&cli_debug)
        .resolve();

    if resolved_debug.enabled {
        println!("🔍 Debug mode enabled (level: {:?})", resolved_debug.level);
        if resolved_debug.delay > 0 {
            println!("   Step delay: {}ms", resolved_debug.delay);
        }
        if resolved_debug.highlight {
            println!("   Element highlighting: on");
        }
        if resolved_debug.capture != CaptureMode::None {
            println!("   Screenshot capture: {:?}", resolved_debug.capture);
        }
    }

    println!("✓ Workflow '{}' loaded successfully", workflow.name);
    println!(
        "  Description: {}",
        workflow.description.as_deref().unwrap_or("(none)")
    );
    println!("  Steps: {}", workflow.steps.len());

    // Show steps
    for (i, step) in workflow.steps.iter().enumerate() {
        let id = step.id.as_deref().unwrap_or("-");
        println!("    {}. [{}] {}", i + 1, id, step.action);
    }

    println!("\n🚀 Launching browser...");

    // Build browser config
    use chromiumoxide::browser::{Browser, BrowserConfig};

    let headless = workflow.browser.headless;

    // Create temp user data dir to ensure clean profile
    let temp_profile =
        std::env::temp_dir().join(format!("automodus-workflow-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_profile);

    let mut browser_config = BrowserConfig::builder();

    if !headless {
        browser_config = browser_config.with_head();
    }

    // Add stability args
    browser_config = browser_config
        .arg("--no-sandbox")
        .arg("--disable-setuid-sandbox")
        .arg("--disable-dev-shm-usage")
        .arg("--disable-web-security")
        .arg("--disable-extensions")
        .arg("--disable-gpu")
        .arg("--no-first-run")
        .arg("--disable-session-crashed-bubble")
        .arg("--disable-infobars")
        .arg(format!("--user-data-dir={}", temp_profile.display()));

    let config = browser_config
        .build()
        .map_err(|e| format!("Failed to build browser config: {}", e))?;

    // Launch browser
    let (browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| format!("Failed to launch browser: {}", e))?;

    // Spawn handler task
    tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if let Err(e) = h {
                debug!("Browser handler event: {:?}", e);
                if e.to_string().contains("connection closed") {
                    break;
                }
            }
        }
    });

    println!("✓ Browser launched");

    // Get a page
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let pages = browser
        .pages()
        .await
        .map_err(|e| format!("Failed to get pages: {}", e))?;

    let page = if pages.is_empty() {
        browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?
    } else {
        pages.into_iter().next().unwrap()
    };

    println!("✓ Browser page ready");

    // Create adapter and engine
    let adapter = ChromePageAdapter::new(page);
    let engine = WorkflowEngine::new();

    println!("\n▶ Executing workflow...\n");

    // Build params from workflow defaults
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    for (name, def) in &workflow.params {
        if let Some(default) = &def.default {
            params.insert(name.clone(), yaml_to_json(default));
        }
    }

    if !params.is_empty() {
        println!("  Params: {:?}", params.keys().collect::<Vec<_>>());
    }
    let result = engine
        .execute_with_debug(&workflow, &adapter, params, resolved_debug)
        .await
        .map_err(|e| format!("Workflow execution failed: {}", e))?;

    // Print result
    println!("\n{}", "─".repeat(50));
    if result.success {
        println!("✅ Workflow completed successfully!");
    } else {
        println!("❌ Workflow failed: {}", result.error.unwrap_or_default());
    }

    println!("  Duration: {}ms", result.duration_ms);
    println!("  Steps executed: {}", result.steps_executed);

    // Show debug screenshots if any were captured
    if !result.debug_screenshots.is_empty() {
        println!("  Debug screenshots: {}", result.debug_screenshots.len());
        for path in &result.debug_screenshots {
            println!("    - {}", path);
        }
    }

    if !result.output.is_null()
        && result
            .output
            .as_object()
            .map(|o| !o.is_empty())
            .unwrap_or(true)
    {
        println!(
            "  Output: {}",
            serde_json::to_string_pretty(&result.output)?
        );
    }

    if !result.events.is_empty() {
        println!("  Events emitted: {}", result.events.len());
        for (event, _) in &result.events {
            println!("    - {}", event);
        }
    }

    // Keep browser open based on flag
    if keep_open {
        println!("\n📌 Browser kept open. Press Ctrl+C to exit.");
        // Wait indefinitely until Ctrl+C
        tokio::signal::ctrl_c().await?;
        println!("\nClosing browser...");
    } else {
        println!("\nBrowser will close in 5 seconds...");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    Ok(())
}

/// Interactive shell mode - keeps browser running for multiple flow executions
async fn run_shell() -> Result<(), Box<dyn std::error::Error>> {
    use chromiumoxide::browser::{Browser, BrowserConfig};
    use std::io::{self, BufRead, Write};

    println!("🚀 Starting interactive shell mode...\n");
    println!("Launching browser (headless: false)...");

    // Create temp user data dir to ensure clean profile
    let temp_profile = std::env::temp_dir().join(format!("automodus-shell-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_profile);

    // Launch browser with head
    let browser_config = BrowserConfig::builder()
        .with_head()
        .arg("--no-sandbox")
        .arg("--disable-setuid-sandbox")
        .arg("--disable-dev-shm-usage")
        .arg("--disable-web-security")
        .arg("--disable-extensions")
        .arg("--disable-gpu")
        .arg("--no-first-run")
        .arg("--disable-session-crashed-bubble")
        .arg("--disable-infobars")
        .arg(format!("--user-data-dir={}", temp_profile.display()))
        .build()
        .map_err(|e| format!("Failed to build browser config: {}", e))?;

    let (browser, mut handler) = Browser::launch(browser_config)
        .await
        .map_err(|e| format!("Failed to launch browser: {}", e))?;

    // Spawn handler task
    tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if let Err(e) = h {
                debug!("Browser handler event: {:?}", e);
                if e.to_string().contains("connection closed") {
                    break;
                }
            }
        }
    });

    // Get initial page
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let pages = browser
        .pages()
        .await
        .map_err(|e| format!("Failed to get pages: {}", e))?;

    let page = if pages.is_empty() {
        browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?
    } else {
        pages.into_iter().next().unwrap()
    };

    println!("✓ Browser ready!\n");

    // Create adapter and engine (reused across flows)
    let adapter = ChromePageAdapter::new(page);
    let engine = WorkflowEngine::new();

    println!("Commands:");
    println!("  run <workflow.yaml>     Run a workflow file");
    println!("  list                List available workflows");
    println!("  goto <url>          Navigate to URL");
    println!("  status              Check page status");
    println!("  quit / exit         Exit shell\n");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("automodus> ");
        stdout.flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break; // EOF
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        let cmd = parts.first().map(|s| *s).unwrap_or("");

        match cmd {
            "quit" | "exit" | "q" => {
                println!("Closing browser...");
                break;
            }
            "run" | "r" => {
                if parts.len() < 2 {
                    println!("Usage: run <workflow.yaml> [param=value ...]");
                    continue;
                }

                let path = std::path::Path::new(parts[1]);

                // Parse additional params - handle quoted values
                let mut params: HashMap<String, serde_json::Value> = HashMap::new();
                // Rejoin remaining args and parse properly to handle quotes
                let args_str = parts[2..].join(" ");
                for param in parse_shell_params(&args_str) {
                    if let Some((key, value)) = param.split_once('=') {
                        // Strip surrounding quotes from value if present
                        let value = value.trim();
                        let value = if (value.starts_with('"') && value.ends_with('"'))
                            || (value.starts_with('\'') && value.ends_with('\''))
                        {
                            &value[1..value.len() - 1]
                        } else {
                            value
                        };
                        params.insert(
                            key.to_string(),
                            serde_json::Value::String(value.to_string()),
                        );
                    }
                }

                match run_workflow_with_adapter(path, &adapter, &engine, params).await {
                    Ok(_) => {}
                    Err(e) => println!("❌ Error: {}", e),
                }
            }
            "list" | "ls" => {
                let workflows_dir = std::env::var("AUTOMODUS_WORKFLOWS")
                    .unwrap_or_else(|_| "workflows".to_string());
                println!("Workflows in {}:", workflows_dir);

                if let Ok(entries) = glob::glob(&format!("{}/**/*.yaml", workflows_dir)) {
                    for entry in entries.flatten() {
                        println!("  {}", entry.display());
                    }
                }
            }
            "goto" | "g" => {
                if parts.len() < 2 {
                    println!("Usage: goto <url>");
                    continue;
                }

                let url = parts[1];
                match adapter.goto(url).await {
                    Ok(_) => println!("✓ Navigated to {}", url),
                    Err(e) => println!("❌ Navigation failed: {}", e),
                }
            }
            "status" | "s" => match adapter.current_url().await {
                Ok(url) => println!("  URL: {}", url),
                Err(e) => println!("  URL: (error: {})", e),
            },
            "help" | "h" | "?" => {
                println!("Commands:");
                println!("  run <workflow.yaml>     Run a workflow file");
                println!("  list                List available workflows");
                println!("  goto <url>          Navigate to URL");
                println!("  status              Check page status");
                println!("  quit / exit         Exit shell");
            }
            _ => {
                println!("Unknown command: {}. Type 'help' for commands.", cmd);
            }
        }
    }

    Ok(())
}

/// Run a workflow using an existing adapter (for shell mode)
async fn run_workflow_with_adapter(
    path: &std::path::Path,
    adapter: &ChromePageAdapter,
    engine: &WorkflowEngine,
    extra_params: HashMap<String, serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Loading workflow from: {}", path.display());

    let content = std::fs::read_to_string(path)?;
    let workflow = WorkflowParser::parse(&content)?;

    println!(
        "✓ Workflow '{}' loaded ({} steps)",
        workflow.name,
        workflow.steps.len()
    );
    println!("▶ Executing...\n");

    // Build params from workflow defaults + extra params
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    for (name, def) in &workflow.params {
        if let Some(default) = &def.default {
            params.insert(name.clone(), yaml_to_json(default));
        }
    }
    // Override with extra params
    params.extend(extra_params);

    let result = engine
        .execute(&workflow, adapter, params)
        .await
        .map_err(|e| format!("Workflow execution failed: {}", e))?;

    println!("{}", "─".repeat(50));
    if result.success {
        println!("✅ Workflow completed successfully!");
    } else {
        println!("❌ Workflow failed: {}", result.error.unwrap_or_default());
    }

    println!("  Duration: {}ms", result.duration_ms);
    println!("  Steps executed: {}", result.steps_executed);

    if !result.output.is_null()
        && result
            .output
            .as_object()
            .map(|o| !o.is_empty())
            .unwrap_or(true)
    {
        println!(
            "  Output: {}",
            serde_json::to_string_pretty(&result.output)?
        );
    }

    Ok(())
}

fn validate_workflows(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("Validating workflows in: {}", path.display());

    let mut valid = 0;
    let mut invalid = 0;

    if path.is_file() {
        match validate_single_workflow(path) {
            Ok(_) => {
                println!("  ✓ {}", path.display());
                valid += 1;
            }
            Err(e) => {
                println!("  ✗ {}: {}", path.display(), e);
                invalid += 1;
            }
        }
    } else {
        for entry in glob::glob(&format!("{}/**/*.yaml", path.display()))? {
            match entry {
                Ok(file_path) => match validate_single_workflow(&file_path) {
                    Ok(_) => {
                        println!("  ✓ {}", file_path.display());
                        valid += 1;
                    }
                    Err(e) => {
                        println!("  ✗ {}: {}", file_path.display(), e);
                        invalid += 1;
                    }
                },
                Err(e) => {
                    eprintln!("  ? Error reading path: {}", e);
                }
            }
        }

        for entry in glob::glob(&format!("{}/**/*.yml", path.display()))? {
            if let Ok(file_path) = entry {
                match validate_single_workflow(&file_path) {
                    Ok(_) => {
                        println!("  ✓ {}", file_path.display());
                        valid += 1;
                    }
                    Err(e) => {
                        println!("  ✗ {}: {}", file_path.display(), e);
                        invalid += 1;
                    }
                }
            }
        }
    }

    println!();
    println!("Results: {} valid, {} invalid", valid, invalid);

    if invalid > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn validate_single_workflow(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let workflow = WorkflowParser::parse(&content)?;
    WorkflowParser::validate(&workflow)?;
    Ok(())
}

async fn list_workflows() -> Result<(), Box<dyn std::error::Error>> {
    let workflows_dir =
        std::env::var("AUTOMODUS_WORKFLOWS").unwrap_or_else(|_| "workflows".to_string());

    println!("Workflows in {}:\n", workflows_dir);

    let loader = WorkflowLoader::new(&workflows_dir);
    let loaded = loader.load_all().await?;

    if loaded.is_empty() {
        println!("  (no workflows found)");
        return Ok(());
    }

    for name in loader.list().await {
        if let Some(workflow) = loader.get(&name).await {
            let triggers = describe_triggers(&workflow);
            println!(
                "  {} - {} ({})",
                name,
                workflow.description.as_deref().unwrap_or("No description"),
                triggers
            );
        }
    }

    println!("\nTotal: {} workflow(s)", loaded.len());

    Ok(())
}

fn describe_triggers(workflow: &automodus::workflow::Workflow) -> String {
    let mut triggers = Vec::new();

    if workflow.triggers.api.is_some() {
        triggers.push("API");
    }
    if workflow.triggers.schedule.is_some() {
        triggers.push("Schedule");
    }
    if workflow.triggers.event.is_some() {
        triggers.push("Event");
    }
    if workflow.triggers.webhook.is_some() {
        triggers.push("Webhook");
    }
    if workflow.triggers.manual {
        triggers.push("Manual");
    }

    if triggers.is_empty() {
        "Manual".to_string()
    } else {
        triggers.join(", ")
    }
}
