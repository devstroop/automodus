//! Automodus - Programmable Workflow Automation Platform
//!
//! CLI for running YAML-based automation workflows.
//!
//! ## Usage
//!
//! ```bash
//! # Run a workflow
//! automodus run ../examples/browser/search_form.yaml
//! automodus run ../examples/whatsapp/whatsapp.yaml action=check_status
//!
//! # Start the API server
//! automodus serve
//!
//! # Validate workflows
//! automodus validate ../examples/
//! ```

use futures_util::FutureExt;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use automodus::{
    actions::BrowserHandle,
    core::{AppCore, ShellPauseHandler, WorkflowEngine},
    daemon::{Daemon, DaemonConfig, DaemonStatus},
    modules::ChromePageAdapter,
    shell::{ShellClient, ShellCommand, ShellConfig},
    utils::{logging, yaml_to_json},
    workflow::{
        schema::{CaptureMode, DebugConfig, DebugProfile, LogLevel, ResolvedDebugConfig},
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
        /// Extra params as key=value CLI args (override workflow defaults)
        params: HashMap<String, String>,
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
    /// Internal: run daemon in foreground (used by background spawn)
    Run,
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

            // Parse debug flags and key=value params
            let mut debug = DebugConfig::default();
            let mut params: HashMap<String, String> = HashMap::new();

            for arg in &args[3..] {
                if arg.starts_with("--") || arg == "-d" || arg == "-k" {
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
                } else if let Some((key, value)) = arg.split_once('=') {
                    // key=value workflow params (e.g. action=check_status)
                    if !key.is_empty() {
                        params.insert(key.to_string(), value.to_string());
                    }
                }
            }

            Command::Run {
                path: PathBuf::from(&args[2]),
                keep_open,
                debug,
                params,
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
                "__run__" => DaemonOp::Run,
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
    run <workflow.yaml> [key=value ...]
                        Run a workflow; key=value overrides param defaults
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
    automodus run ../examples/browser/search_form.yaml

    # Run workflow with debug mode
    automodus run ../examples/compose/pipeline.yaml --debug

    # Run with verbose profile and custom delay
    automodus run ../examples/http/http_api.yaml --profile=verbose --delay=1000

    # Run with screenshots on failure (for CI)
    automodus run ../examples/http/http_api.yaml --capture=failure

    # Run workflow and keep browser open
    automodus run ../examples/compose/pipeline.yaml --keep-open

    # Start the daemon
    automodus daemon start

    # Check daemon status
    automodus daemon status

    # View daemon logs (follow mode)
    automodus daemon logs -f

    # Start interactive shell for testing
    automodus shell

    # Validate all workflows in a directory
    automodus validate ../examples/

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
            params,
        } => {
            print_banner();
            run_workflow(&path, keep_open, debug, params).await?;
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
            
            let mut daemon = Daemon::new(config.clone());
            
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
            
            // Route through daemon architecture (foreground)
            daemon.start().await.map_err(|e| format!("Failed to start daemon: {}", e))?;
            daemon.run().await.map_err(|e| format!("Daemon error: {}", e))?;
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

        DaemonOp::Run => {
            // Internal: run daemon in foreground (spawned by `daemon start`)
            let mut daemon = Daemon::new(config.clone());
            daemon.start().await.map_err(|e| format!("Failed to start daemon: {}", e))?;
            daemon.run().await.map_err(|e| format!("Daemon error: {}", e))?;
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
    cli_params: HashMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Loading workflow from: {}", path.display());

    // Parse workflow
    let content = std::fs::read_to_string(path)?;
    let workflow = WorkflowParser::parse(&content)?;

    // Merge debug configs: env (lowest) → workflow → CLI (highest)
    // Note: workflow.debug.profile is not applied here (run path skips with_profile);
    // use CLI --profile= instead.
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

    // Launch browser using shared launch helper
    use automodus::modules::browser::launch::{launch_session, LaunchOptions};
    use std::sync::Arc;

    let headless = workflow.browser.headless;
    let mut options = LaunchOptions::for_workflow().headless(headless);
    // Respect browser.engine from config; unsupported engines fail fast in launch_session.
    if let Ok(cfg) = automodus::config::AppConfig::load() {
        options = options.engine(cfg.browser.engine);
    }
    let adapter = launch_session(&options)
        .await
        .map_err(|e| format!("Failed to launch browser: {}", e))?;

    println!("✓ Browser launched");
    println!("✓ Browser page ready");

    if let Err(e) = adapter.start_console_listener().await {
        eprintln!("Warning: failed to start console listener: {}", e);
    }
    if let Err(e) = adapter.start_network_listener().await {
        eprintln!("Warning: failed to start network listener: {}", e);
    }
    if let Err(e) = adapter.start_crash_listener().await {
        eprintln!("Warning: failed to start crash listener: {}", e);
    }
    // Prefer AUTOMODUS_WORKFLOWS for `call` resolution; fall back to the file's directory
    let workflows_dir = std::env::var("AUTOMODUS_WORKFLOWS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            path.parent()
                .unwrap_or(std::path::Path::new("workflows"))
                .to_path_buf()
        });
    let loader = Arc::new(WorkflowLoader::new(&workflows_dir));
    let engine = WorkflowEngine::with_resolver(loader);

    println!("\n▶ Executing workflow...\n");

    // Build params from workflow defaults, then override with CLI key=value params
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    for (name, def) in &workflow.params {
        if let Some(default) = &def.default {
            params.insert(name.clone(), yaml_to_json(default));
        }
    }
    for (key, value) in cli_params {
        // Coerce plain numbers/bools so numeric params (user_id, etc.) work
        let coerced = match value.as_str() {
            "true" => serde_json::Value::Bool(true),
            "false" => serde_json::Value::Bool(false),
            v if v.parse::<i64>().is_ok() => {
                serde_json::Number::from_str(v)
                    .map(serde_json::Value::Number)
                    .unwrap_or_else(|_| serde_json::Value::String(v.to_string()))
            }
            v => serde_json::Value::String(v.to_string()),
        };
        params.insert(key, coerced);
    }

    if !params.is_empty() {
        println!("  Params: {:?}", params.keys().collect::<Vec<_>>());
    }
    let result = engine
        .execute_with_pause_handler(&workflow, &adapter, params, resolved_debug, &ShellPauseHandler, None)
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
    use rustyline::error::ReadlineError;

    println!("🚀 Starting interactive shell mode...\n");

    // Try connecting to running daemon first
    let daemon_config = DaemonConfig::default();
    if let Ok(mut client) = automodus::daemon::connect_to_daemon(&daemon_config).await {
        if client.ping().await.is_ok() {
            println!("✓ Connected to daemon\n");
            return run_shell_daemon(client).await;
        }
    }

    // Standalone mode — no daemon running
    println!("ℹ️  No daemon running. Starting standalone mode.");
    println!("   (Start daemon with 'automodus daemon start' for shared sessions)\n");

    use automodus::modules::browser::launch::{launch_session, LaunchOptions};
    use std::sync::{Arc, RwLock};

    println!("Launching browser (headless: false)...");
    let mut options = LaunchOptions::for_shell();
    // Respect browser.engine from config; unsupported engines fail fast in launch_session.
    if let Ok(cfg) = automodus::config::AppConfig::load() {
        options = options.engine(cfg.browser.engine);
    }
    let adapter = launch_session(&options)
        .await
        .map_err(|e| format!("Failed to launch browser: {}", e))?;

    println!("✓ Browser ready!\n");

    if let Err(e) = adapter.start_console_listener().await {
        eprintln!("Warning: failed to start console listener: {}", e);
    }
    if let Err(e) = adapter.start_network_listener().await {
        eprintln!("Warning: failed to start network listener: {}", e);
    }
    if let Err(e) = adapter.start_crash_listener().await {
        eprintln!("Warning: failed to start crash listener: {}", e);
    }
    let workflows_dir = std::env::var("AUTOMODUS_WORKFLOWS")
        .unwrap_or_else(|_| "workflows".to_string());
    let loader = Arc::new(WorkflowLoader::new(&workflows_dir));
    let engine = WorkflowEngine::with_resolver(loader);

    // Create AppCore for session management
    let daemon_config = DaemonConfig::default();
    let core = Arc::new(AppCore::new(&daemon_config));

    // Create initial session
    let initial_session_id = core
        .create_session(Some("default".to_string()))
        .await
        .map_err(|e| format!("Failed to create initial session: {}", e))?;
    let mut current_session_id = initial_session_id;

    // Shared session names for completer
    let session_names: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(vec!["default".to_string()]));

    // Create ShellClient with rustyline (history, completion, line editing)
    let shell_config = ShellConfig::default();
    let mut shell = ShellClient::with_session_names(shell_config, session_names.clone())
        .map_err(|e| format!("Failed to create shell: {}", e))?;

    // Helper: refresh session names for completer
    let refresh_session_names = |core: &Arc<AppCore>, names: &Arc<RwLock<Vec<String>>>| {
        let core = core.clone();
        let names = names.clone();
        async move {
            let sessions = core.list_sessions().await;
            let new_names: Vec<String> = sessions
                .iter()
                .filter_map(|s| s.name.clone())
                .chain(sessions.iter().map(|s| s.id[..8].to_string()))
                .collect();
            if let Ok(mut guard) = names.write() {
                *guard = new_names;
            }
        }
    };

    // Helper: build prompt with session name
    let build_prompt = |core: &Arc<AppCore>, session_id: &str| {
        let core = core.clone();
        let session_id = session_id.to_string();
        async move {
            if let Some(session) = core.get_session(&session_id).await {
                let label = session
                    .name
                    .unwrap_or_else(|| session_id[..8].to_string());
                format!("automodus [{}]> ", label)
            } else {
                "automodus> ".to_string()
            }
        }
    };

    ShellClient::print_help();

    loop {
        let prompt = build_prompt(&core, &current_session_id).await;
        let line = match shell.readline_with_prompt(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("Ctrl+C — type 'quit' to exit");
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Shell error: {}", e);
                break;
            }
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Touch current session on any command
        core.touch_session(&current_session_id).await;

        match ShellClient::parse_command(line) {
            ShellCommand::Quit => {
                println!("Closing browser...");
                break;
            }
            ShellCommand::Help => {
                ShellClient::print_help();
            }
            ShellCommand::SessionNew { name, keep_alive } => {
                match core.create_session(name.clone()).await {
                    Ok(id) => {
                        if keep_alive {
                            let _ = core.set_session_keep_alive(&id, true).await;
                        }
                        let label = name.as_deref().unwrap_or(&id[..8]);
                        println!("✓ Session created: {} ({})", label, &id[..8]);
                        current_session_id = id;
                        refresh_session_names(&core, &session_names).await;
                    }
                    Err(e) => println!("❌ Failed to create session: {}", e),
                }
            }
            ShellCommand::SessionList => {
                let sessions = core.list_sessions().await;
                if sessions.is_empty() {
                    println!("  (no sessions)");
                } else {
                    println!("  {:>8}  {:<16}  {:<10}  {}", "ID", "Name", "Keep-Alive", "Last Activity");
                    println!("  {}  {}  {}  {}", "─".repeat(8), "─".repeat(16), "─".repeat(10), "─".repeat(20));
                    for s in &sessions {
                        let marker = if s.id == current_session_id { "→ " } else { "  " };
                        let name = s.name.as_deref().unwrap_or("-");
                        let ka = if s.keep_alive { "yes" } else { "no" };
                        let age = chrono::Utc::now() - s.last_activity;
                        let age_str = if age.num_seconds() < 60 {
                            format!("{}s ago", age.num_seconds())
                        } else if age.num_minutes() < 60 {
                            format!("{}m ago", age.num_minutes())
                        } else {
                            format!("{}h ago", age.num_hours())
                        };
                        println!("{}{:>8}  {:<16}  {:<10}  {}", marker, &s.id[..8], name, ka, age_str);
                    }
                }
            }
            ShellCommand::SessionSwitch { target } => {
                match core.find_session(&target).await {
                    Some(session) => {
                        current_session_id = session.id.clone();
                        let label = session.name.as_deref().unwrap_or(&session.id[..8]);
                        println!("✓ Switched to session: {}", label);
                    }
                    None => {
                        // Try prefix match on ID
                        let sessions = core.list_sessions().await;
                        let matches: Vec<_> = sessions.iter().filter(|s| s.id.starts_with(&target)).collect();
                        match matches.len() {
                            1 => {
                                current_session_id = matches[0].id.clone();
                                let label = matches[0].name.as_deref().unwrap_or(&matches[0].id[..8]);
                                println!("✓ Switched to session: {}", label);
                            }
                            0 => println!("❌ No session found matching '{}'", target),
                            n => println!("❌ Ambiguous: {} sessions match '{}'. Be more specific.", n, target),
                        }
                    }
                }
            }
            ShellCommand::SessionClose { target } => {
                let close_id = if let Some(ref t) = target {
                    match core.find_session(t).await {
                        Some(s) => s.id.clone(),
                        None => {
                            // Try prefix match
                            let sessions = core.list_sessions().await;
                            let matches: Vec<_> = sessions.iter().filter(|s| s.id.starts_with(t)).collect();
                            if matches.len() == 1 {
                                matches[0].id.clone()
                            } else {
                                println!("❌ No session found matching '{}'", t);
                                continue;
                            }
                        }
                    }
                } else {
                    current_session_id.clone()
                };

                match core.close_session(&close_id).await {
                    Ok(_) => {
                        println!("✓ Session closed: {}", &close_id[..8]);
                        if close_id == current_session_id {
                            // Switch to another session or create new default
                            let sessions = core.list_sessions().await;
                            if let Some(next) = sessions.first() {
                                current_session_id = next.id.clone();
                            } else {
                                match core.create_session(Some("default".to_string())).await {
                                    Ok(id) => current_session_id = id,
                                    Err(e) => {
                                        eprintln!("Failed to create fallback session: {}", e);
                                        break;
                                    }
                                }
                            }
                        }
                        refresh_session_names(&core, &session_names).await;
                    }
                    Err(e) => println!("❌ Failed to close session: {}", e),
                }
            }
            ShellCommand::SessionInfo => {
                match core.get_session(&current_session_id).await {
                    Some(session) => {
                        let age = chrono::Utc::now() - session.created_at;
                        let idle = chrono::Utc::now() - session.last_activity;
                        println!("  Session ID:   {}", session.id);
                        println!("  Name:         {}", session.name.as_deref().unwrap_or("-"));
                        println!("  Keep-Alive:   {}", if session.keep_alive { "yes" } else { "no" });
                        println!("  Created:      {} ({} ago)", session.created_at.format("%H:%M:%S"), format_duration(age));
                        println!("  Last Active:  {} ({} ago)", session.last_activity.format("%H:%M:%S"), format_duration(idle));
                    }
                    None => println!("❌ Current session not found (stale)"),
                }
            }
            ShellCommand::SessionKeepAlive { target, toggle } => {
                let target_id = if let Some(ref t) = target {
                    match core.find_session(t).await {
                        Some(s) => s.id.clone(),
                        None => {
                            println!("❌ No session found matching '{}'", t);
                            continue;
                        }
                    }
                } else {
                    current_session_id.clone()
                };

                // If no toggle specified, flip current value
                let new_val = if let Some(val) = toggle {
                    val
                } else {
                    match core.get_session(&target_id).await {
                        Some(s) => !s.keep_alive,
                        None => {
                            println!("❌ Session not found");
                            continue;
                        }
                    }
                };

                match core.set_session_keep_alive(&target_id, new_val).await {
                    Ok(_) => println!("✓ Keep-alive {}: {}", if new_val { "enabled" } else { "disabled" }, &target_id[..8]),
                    Err(e) => println!("❌ Failed: {}", e),
                }
            }
            ShellCommand::Goto { url } => {
                if url.is_empty() {
                    println!("Usage: goto <url>");
                    continue;
                }
                match adapter.goto(&url).await {
                    Ok(_) => println!("✓ Navigated to {}", url),
                    Err(e) => println!("❌ Navigation failed: {}", e),
                }
            }
            ShellCommand::Click { selector } => {
                if selector.is_empty() {
                    println!("Usage: click <selector>");
                    continue;
                }
                match adapter.click(&selector).await {
                    Ok(_) => println!("✓ Clicked {}", selector),
                    Err(e) => println!("❌ Click failed: {}", e),
                }
            }
            ShellCommand::Type { selector, text } => {
                match adapter.type_text(&selector, &text, true).await {
                    Ok(_) => println!("✓ Typed into {}", selector),
                    Err(e) => println!("❌ Type failed: {}", e),
                }
            }
            ShellCommand::Wait { selector, timeout } => {
                if selector.is_empty() {
                    println!("Usage: wait <selector> [timeout_ms]");
                    continue;
                }
                let timeout_ms = timeout.unwrap_or(5000);
                match adapter.wait_for(&selector, timeout_ms).await {
                    Ok(_) => println!("✓ Element found: {}", selector),
                    Err(e) => println!("❌ Wait failed: {}", e),
                }
            }
            ShellCommand::Text { selector } => {
                if selector.is_empty() {
                    println!("Usage: text <selector>");
                    continue;
                }
                match adapter.get_text(&selector).await {
                    Ok(text) => println!("{}", text),
                    Err(e) => println!("❌ Text extraction failed: {}", e),
                }
            }
            ShellCommand::Screenshot { path } => {
                match adapter.screenshot(false).await {
                    Ok(data) => {
                        let path = path.unwrap_or_else(|| {
                            PathBuf::from(format!(
                                "screenshot_{}.png",
                                chrono::Utc::now().format("%Y%m%d_%H%M%S")
                            ))
                        });
                        match std::fs::write(&path, &data) {
                            Ok(_) => println!("✓ Screenshot saved to {}", path.display()),
                            Err(e) => println!("❌ Failed to save screenshot: {}", e),
                        }
                    }
                    Err(e) => println!("❌ Screenshot failed: {}", e),
                }
            }
            ShellCommand::Eval { script } => {
                if script.is_empty() {
                    println!("Usage: eval <javascript>");
                    continue;
                }
                match adapter.eval(&script).await {
                    Ok(result) => println!("{}", serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())),
                    Err(e) => println!("❌ Eval failed: {}", e),
                }
            }
            ShellCommand::Status => {
                match adapter.current_url().await {
                    Ok(url) => println!("  URL: {}", url),
                    Err(e) => println!("  URL: (error: {})", e),
                }
            }
            ShellCommand::Back => {
                match adapter.back().await {
                    Ok(_) => println!("✓ Navigated back"),
                    Err(e) => println!("❌ Back failed: {}", e),
                }
            }
            ShellCommand::Forward => {
                match adapter.forward().await {
                    Ok(_) => println!("✓ Navigated forward"),
                    Err(e) => println!("❌ Forward failed: {}", e),
                }
            }
            ShellCommand::Refresh => {
                match adapter.reload().await {
                    Ok(_) => println!("✓ Page refreshed"),
                    Err(e) => println!("❌ Refresh failed: {}", e),
                }
            }
            ShellCommand::Highlight { selector } => {
                if selector.is_empty() {
                    println!("Usage: highlight <selector>");
                    continue;
                }
                let js = format!(
                    r#"(function() {{
                        const el = document.querySelector('{}');
                        if (!el) return 'not found';
                        el.style.outline = '3px solid red';
                        el.style.outlineOffset = '2px';
                        setTimeout(() => {{ el.style.outline = ''; el.style.outlineOffset = ''; }}, 3000);
                        return 'highlighted';
                    }})()
                    "#,
                    selector.replace('\\', "\\\\").replace('\'', "\\'")
                );
                match adapter.eval(&js).await {
                    Ok(val) => {
                        let result = val.as_str().unwrap_or("done");
                        if result == "not found" {
                            println!("❌ Element not found: {}", selector);
                        } else {
                            println!("✓ Highlighted {} (3s)", selector);
                        }
                    }
                    Err(e) => println!("❌ Highlight failed: {}", e),
                }
            }
            ShellCommand::Find { selector } => {
                if selector.is_empty() {
                    println!("Usage: find <selector>");
                    continue;
                }
                let js = format!(
                    r#"(function() {{
                        var els = document.querySelectorAll('{}');
                        var results = [];
                        for (var i = 0; i < Math.min(els.length, 10); i++) {{
                            var el = els[i];
                            results.push({{
                                tag: el.tagName.toLowerCase(),
                                id: el.id || null,
                                classes: el.className || null,
                                text: (el.textContent || '').trim().substring(0, 100)
                            }});
                        }}
                        return JSON.stringify({{ count: els.length, matches: results }});
                    }})()
                    "#,
                    selector.replace('\\', "\\\\").replace('\'', "\\'")
                );
                match adapter.eval(&js).await {
                    Ok(val) => {
                        let json_str = val.as_str().unwrap_or("{}");
                        match serde_json::from_str::<serde_json::Value>(json_str) {
                            Ok(data) => {
                                let count = data["count"].as_u64().unwrap_or(0);
                                if count == 0 {
                                    println!("  No elements found matching '{}'", selector);
                                } else {
                                    println!("  Found {} element(s) matching '{}'", count, selector);
                                    if let Some(matches) = data["matches"].as_array() {
                                        for (i, m) in matches.iter().enumerate() {
                                            let tag = m["tag"].as_str().unwrap_or("?");
                                            let id = m["id"].as_str().filter(|s| !s.is_empty());
                                            let classes = m["classes"].as_str().filter(|s| !s.is_empty());
                                            let text = m["text"].as_str().unwrap_or("");
                                            let mut desc = format!("<{}", tag);
                                            if let Some(id) = id {
                                                desc.push_str(&format!(" id=\"{}\"", id));
                                            }
                                            if let Some(cls) = classes {
                                                desc.push_str(&format!(" class=\"{}\"", cls));
                                            }
                                            desc.push('>');
                                            if !text.is_empty() {
                                                desc.push_str(&format!(" \"{}\"", text));
                                            }
                                            println!("    [{}] {}", i, desc);
                                        }
                                        if count > 10 {
                                            println!("    ... and {} more", count - 10);
                                        }
                                    }
                                }
                            }
                            Err(_) => println!("  No elements found matching '{}'", selector),
                        }
                    }
                    Err(e) => println!("❌ Find failed: {}", e),
                }
            }
            ShellCommand::Run { path, params } => {
                let json_params: HashMap<String, serde_json::Value> = params
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect();
                match run_workflow_with_adapter(&path, &adapter, &engine, json_params).await {
                    Ok(_) => {}
                    Err(e) => println!("❌ Error: {}", e),
                }
            }
            ShellCommand::Trace { path, params } => {
                println!("🔍 Running with trace logging...");
                let json_params: HashMap<String, serde_json::Value> = params
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect();
                // Run with trace-level debug
                match run_workflow_with_adapter(&path, &adapter, &engine, json_params).await {
                    Ok(_) => {}
                    Err(e) => println!("❌ Trace error: {}", e),
                }
            }
            ShellCommand::List => {
                let workflows_dir = std::env::var("AUTOMODUS_WORKFLOWS")
                    .unwrap_or_else(|_| "workflows".to_string());
                println!("Workflows in {}:", workflows_dir);
                if let Ok(entries) = glob::glob(&format!("{}/**/*.yaml", workflows_dir)) {
                    for entry in entries.flatten() {
                        println!("  {}", entry.display());
                    }
                }
            }
            ShellCommand::DebugOn { profile } => {
                if let Some(p) = &profile {
                    println!("✓ Debug mode ON (profile: {})", p);
                } else {
                    println!("✓ Debug mode ON");
                }
            }
            ShellCommand::DebugOff => {
                println!("✓ Debug mode OFF");
            }
            ShellCommand::DebugStatus => {
                println!("  Debug: off (toggle with 'debug on')");
            }
            ShellCommand::DebugClean => {
                let debug_dir = std::path::Path::new("data/debug");
                let policy = automodus::utils::CleanupPolicy::default();
                match automodus::utils::cleanup_debug_dir(debug_dir, &policy) {
                    Ok(stats) => println!("✓ {}", stats),
                    Err(e) => println!("❌ Cleanup failed: {}", e),
                }
            }
            ShellCommand::Tabs => {
                match adapter.list_tabs().await {
                    Ok(tabs) => {
                        println!("Open tabs ({}):", tabs.len());
                        for tab in &tabs {
                            let marker = if tab.active { " *" } else { "  " };
                            println!("{} [{}] {}", marker, tab.index, tab.url);
                        }
                    }
                    Err(e) => println!("❌ Failed to list tabs: {}", e),
                }
            }
            ShellCommand::TabNew { url } => {
                match adapter.new_tab(url.as_deref()).await {
                    Ok(index) => println!("✓ Opened new tab {}", index),
                    Err(e) => println!("❌ Failed to open tab: {}", e),
                }
            }
            ShellCommand::TabSwitch { index } => {
                match adapter.switch_tab(index).await {
                    Ok(()) => println!("✓ Switched to tab {}", index),
                    Err(e) => println!("❌ Failed to switch tab: {}", e),
                }
            }
            ShellCommand::TabClose { index } => {
                let tab_index = index.unwrap_or_else(|| {
                    // Will be resolved to current tab
                    0 // fallback
                });
                let tab_index = if index.is_some() {
                    tab_index
                } else {
                    match adapter.list_tabs().await {
                        Ok(tabs) => tabs.iter().find(|t| t.active).map(|t| t.index).unwrap_or(0),
                        Err(_) => 0,
                    }
                };
                match adapter.close_tab(tab_index).await {
                    Ok(()) => println!("✓ Closed tab {}", tab_index),
                    Err(e) => println!("❌ Failed to close tab: {}", e),
                }
            }
            ShellCommand::Pdf { path } => {
                match adapter.pdf().await {
                    Ok(data) => {
                        let path = path.unwrap_or_else(|| {
                            PathBuf::from(format!(
                                "page_{}.pdf",
                                chrono::Utc::now().format("%Y%m%d_%H%M%S")
                            ))
                        });
                        match std::fs::write(&path, &data) {
                            Ok(_) => println!("✓ PDF saved to {}", path.display()),
                            Err(e) => println!("❌ Failed to save PDF: {}", e),
                        }
                    }
                    Err(e) => println!("❌ PDF export failed: {}", e),
                }
            }
            ShellCommand::Unknown { command } => {
                if !command.is_empty() {
                    println!("Unknown command: '{}'. Type 'help' for commands.", command);
                }
            }
        }
    }

    // Save history before exit
    if let Err(e) = shell.save_history() {
        eprintln!("Warning: {}", e);
    }

    Ok(())
}

/// Interactive shell connected to a running daemon via Unix socket.
///
/// All browser and session operations are routed through the DaemonClient.
async fn run_shell_daemon(
    mut client: automodus::daemon::DaemonClient,
) -> Result<(), Box<dyn std::error::Error>> {
    use rustyline::error::ReadlineError;
    use std::sync::{Arc, RwLock};

    // Create initial session on daemon
    let initial_id = client
        .session_create(Some("default".to_string()), true)
        .await
        .map_err(|e| format!("Failed to create session: {}", e))?;
    let mut current_session_id = initial_id;

    // Session names for tab completion
    let session_names: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(vec!["default".to_string()]));

    let shell_config = ShellConfig::default();
    let mut shell = ShellClient::with_session_names(shell_config, session_names.clone())
        .map_err(|e| format!("Failed to create shell: {}", e))?;

    let session_label = |sessions: &[serde_json::Value], id: &str| -> String {
        sessions
            .iter()
            .find(|s| s["id"].as_str() == Some(id))
            .and_then(|s| s["name"].as_str().map(String::from))
            .unwrap_or_else(|| id.chars().take(8).collect())
    };

    ShellClient::print_help();

    loop {
        // Build prompt with session name
        let prompt = if let Ok(session) = client.session_get(&current_session_id).await {
            let label = session["name"]
                .as_str()
                .unwrap_or(&current_session_id[..8]);
            format!("automodus [{}]> ", label)
        } else {
            "automodus> ".to_string()
        };

        let line = match shell.readline_with_prompt(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("Ctrl+C — type 'quit' to exit");
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Shell error: {}", e);
                break;
            }
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match ShellClient::parse_command(line) {
            ShellCommand::Quit => {
                println!("Disconnecting from daemon...");
                break;
            }
            ShellCommand::Help => {
                ShellClient::print_help();
            }

            // --- Session commands ---
            ShellCommand::SessionNew { name, keep_alive } => {
                match client
                    .session_create(name.clone(), keep_alive)
                    .await
                {
                    Ok(id) => {
                        let label = name.as_deref().unwrap_or(&id[..8]);
                        println!("✓ Session created: {} ({})", label, &id[..8]);
                        current_session_id = id;
                        if let Ok(sessions) = client.session_list().await {
                            let new_names: Vec<String> = sessions.iter().filter_map(|s| {
                                s["name"].as_str().map(String::from)
                                    .or_else(|| s["id"].as_str().map(|id| id[..8].to_string()))
                            }).collect();
                            if let Ok(mut guard) = session_names.write() {
                                *guard = new_names;
                            }
                        }
                    }
                    Err(e) => println!("❌ Failed to create session: {}", e),
                }
            }
            ShellCommand::SessionList => {
                match client.session_list().await {
                    Ok(sessions) => {
                        if sessions.is_empty() {
                            println!("  (no sessions)");
                        } else {
                            println!(
                                "  {:>8}  {:<16}  {:<10}  {}",
                                "ID", "Name", "Keep-Alive", "Last Activity"
                            );
                            println!(
                                "  {}  {}  {}  {}",
                                "─".repeat(8),
                                "─".repeat(16),
                                "─".repeat(10),
                                "─".repeat(20)
                            );
                            for s in &sessions {
                                let id = s["id"].as_str().unwrap_or("");
                                let marker = if id == current_session_id {
                                    "→ "
                                } else {
                                    "  "
                                };
                                let name = s["name"].as_str().unwrap_or("-");
                                let ka = if s["keep_alive"].as_bool().unwrap_or(false) {
                                    "yes"
                                } else {
                                    "no"
                                };
                                let activity =
                                    s["last_activity"].as_str().unwrap_or("unknown");
                                println!(
                                    "{}{:>8}  {:<16}  {:<10}  {}",
                                    marker,
                                    &id[..id.len().min(8)],
                                    name,
                                    ka,
                                    activity
                                );
                            }
                        }
                    }
                    Err(e) => println!("❌ Failed to list sessions: {}", e),
                }
            }
            ShellCommand::SessionSwitch { target } => {
                match client.session_find(&target).await {
                    Ok(session) => {
                        let id = session["id"].as_str().unwrap_or("").to_string();
                        let label = session["name"]
                            .as_str()
                            .unwrap_or(&id[..id.len().min(8)]);
                        println!("✓ Switched to session: {}", label);
                        current_session_id = id;
                    }
                    Err(_) => {
                        // Try prefix match
                        if let Ok(sessions) = client.session_list().await {
                            let matches: Vec<_> = sessions
                                .iter()
                                .filter(|s| {
                                    s["id"]
                                        .as_str()
                                        .map(|id| id.starts_with(&target))
                                        .unwrap_or(false)
                                })
                                .collect();
                            match matches.len() {
                                1 => {
                                    let id =
                                        matches[0]["id"].as_str().unwrap_or("").to_string();
                                    let label = session_label(&sessions, &id);
                                    println!("✓ Switched to session: {}", label);
                                    current_session_id = id;
                                }
                                0 => println!(
                                    "❌ No session found matching '{}'",
                                    target
                                ),
                                n => println!(
                                    "❌ Ambiguous: {} sessions match '{}'. Be more specific.",
                                    n, target
                                ),
                            }
                        } else {
                            println!("❌ No session found matching '{}'", target);
                        }
                    }
                }
            }
            ShellCommand::SessionClose { target } => {
                let close_id = if let Some(ref t) = target {
                    match client.session_find(t).await {
                        Ok(s) => s["id"].as_str().unwrap_or("").to_string(),
                        Err(_) => {
                            if let Ok(sessions) = client.session_list().await {
                                let matches: Vec<_> = sessions
                                    .iter()
                                    .filter(|s| {
                                        s["id"]
                                            .as_str()
                                            .map(|id| id.starts_with(t.as_str()))
                                            .unwrap_or(false)
                                    })
                                    .collect();
                                if matches.len() == 1 {
                                    matches[0]["id"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string()
                                } else {
                                    println!(
                                        "❌ No session found matching '{}'",
                                        t
                                    );
                                    continue;
                                }
                            } else {
                                println!("❌ No session found matching '{}'", t);
                                continue;
                            }
                        }
                    }
                } else {
                    current_session_id.clone()
                };

                match client.session_close(&close_id).await {
                    Ok(_) => {
                        println!(
                            "✓ Session closed: {}",
                            &close_id[..close_id.len().min(8)]
                        );
                        if close_id == current_session_id {
                            if let Ok(sessions) = client.session_list().await {
                                if let Some(next) = sessions.first() {
                                    current_session_id = next["id"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
                                } else {
                                    match client
                                        .session_create(
                                            Some("default".to_string()),
                                            true,
                                        )
                                        .await
                                    {
                                        Ok(id) => current_session_id = id,
                                        Err(e) => {
                                            eprintln!(
                                                "Failed to create fallback session: {}",
                                                e
                                            );
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if let Ok(sessions) = client.session_list().await {
                            let new_names: Vec<String> = sessions.iter().filter_map(|s| {
                                s["name"].as_str().map(String::from)
                                    .or_else(|| s["id"].as_str().map(|id| id[..8].to_string()))
                            }).collect();
                            if let Ok(mut guard) = session_names.write() {
                                *guard = new_names;
                            }
                        }
                    }
                    Err(e) => println!("❌ Failed to close session: {}", e),
                }
            }
            ShellCommand::SessionInfo => {
                match client.session_get(&current_session_id).await {
                    Ok(s) => {
                        println!("  Session ID:   {}", s["id"].as_str().unwrap_or(""));
                        println!("  Name:         {}", s["name"].as_str().unwrap_or("-"));
                        println!(
                            "  Keep-Alive:   {}",
                            if s["keep_alive"].as_bool().unwrap_or(false) {
                                "yes"
                            } else {
                                "no"
                            }
                        );
                        println!(
                            "  Created:      {}",
                            s["created_at"].as_str().unwrap_or("?")
                        );
                        println!(
                            "  Last Active:  {}",
                            s["last_activity"].as_str().unwrap_or("?")
                        );
                    }
                    Err(e) => println!("❌ {}", e),
                }
            }
            ShellCommand::SessionKeepAlive { target, toggle } => {
                let target_id = if let Some(ref t) = target {
                    match client.session_find(t).await {
                        Ok(s) => s["id"].as_str().unwrap_or("").to_string(),
                        Err(_) => {
                            println!("❌ No session found matching '{}'", t);
                            continue;
                        }
                    }
                } else {
                    current_session_id.clone()
                };

                let new_val = if let Some(val) = toggle {
                    val
                } else {
                    match client.session_get(&target_id).await {
                        Ok(s) => !s["keep_alive"].as_bool().unwrap_or(true),
                        Err(_) => {
                            println!("❌ Session not found");
                            continue;
                        }
                    }
                };

                match client
                    .session_set_keep_alive(&target_id, new_val)
                    .await
                {
                    Ok(_) => println!(
                        "✓ Keep-alive {}: {}",
                        if new_val { "enabled" } else { "disabled" },
                        &target_id[..target_id.len().min(8)]
                    ),
                    Err(e) => println!("❌ Failed: {}", e),
                }
            }

            // --- Browser commands ---
            ShellCommand::Goto { url } => {
                if url.is_empty() {
                    println!("Usage: goto <url>");
                    continue;
                }
                match client.browser_goto(&url).await {
                    Ok(_) => println!("✓ Navigated to {}", url),
                    Err(e) => println!("❌ Navigation failed: {}", e),
                }
            }
            ShellCommand::Click { selector } => {
                if selector.is_empty() {
                    println!("Usage: click <selector>");
                    continue;
                }
                match client.browser_click(&selector).await {
                    Ok(_) => println!("✓ Clicked {}", selector),
                    Err(e) => println!("❌ Click failed: {}", e),
                }
            }
            ShellCommand::Type { selector, text } => {
                match client.browser_type(&selector, &text).await {
                    Ok(_) => println!("✓ Typed into {}", selector),
                    Err(e) => println!("❌ Type failed: {}", e),
                }
            }
            ShellCommand::Wait { selector, timeout } => {
                if selector.is_empty() {
                    println!("Usage: wait <selector> [timeout_ms]");
                    continue;
                }
                match client
                    .browser_wait(&selector, timeout)
                    .await
                {
                    Ok(_) => println!("✓ Element found: {}", selector),
                    Err(e) => println!("❌ Wait failed: {}", e),
                }
            }
            ShellCommand::Text { selector } => {
                if selector.is_empty() {
                    println!("Usage: text <selector>");
                    continue;
                }
                match client.browser_get_text(&selector).await {
                    Ok(text) => println!("{}", text),
                    Err(e) => println!("❌ Text extraction failed: {}", e),
                }
            }
            ShellCommand::Screenshot { path } => {
                match client.browser_screenshot().await {
                    Ok(data) => {
                        let path = path.unwrap_or_else(|| {
                            PathBuf::from(format!(
                                "screenshot_{}.png",
                                chrono::Utc::now().format("%Y%m%d_%H%M%S")
                            ))
                        });
                        match std::fs::write(&path, &data) {
                            Ok(_) => {
                                println!("✓ Screenshot saved to {}", path.display())
                            }
                            Err(e) => {
                                println!("❌ Failed to save screenshot: {}", e)
                            }
                        }
                    }
                    Err(e) => println!("❌ Screenshot failed: {}", e),
                }
            }
            ShellCommand::Eval { script } => {
                if script.is_empty() {
                    println!("Usage: eval <javascript>");
                    continue;
                }
                match client.browser_eval(&script).await {
                    Ok(result) => println!(
                        "{}",
                        serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| result.to_string())
                    ),
                    Err(e) => println!("❌ Eval failed: {}", e),
                }
            }
            ShellCommand::Status => {
                match client.status().await {
                    Ok(data) => {
                        let browser =
                            data["browser_running"].as_bool().unwrap_or(false);
                        let sessions = data["sessions"].as_u64().unwrap_or(0);
                        println!("  Mode: daemon");
                        println!(
                            "  Browser: {}",
                            if browser { "running" } else { "stopped" }
                        );
                        println!("  Sessions: {}", sessions);
                        if let Ok(url) = client.browser_get_url().await {
                            println!("  URL: {}", url);
                        }
                    }
                    Err(e) => println!("❌ Status error: {}", e),
                }
            }
            ShellCommand::Back => match client.browser_back().await {
                Ok(_) => println!("✓ Navigated back"),
                Err(e) => println!("❌ Back failed: {}", e),
            },
            ShellCommand::Forward => match client.browser_forward().await {
                Ok(_) => println!("✓ Navigated forward"),
                Err(e) => println!("❌ Forward failed: {}", e),
            },
            ShellCommand::Refresh => match client.browser_reload().await {
                Ok(_) => println!("✓ Page refreshed"),
                Err(e) => println!("❌ Refresh failed: {}", e),
            },
            ShellCommand::Highlight { selector } => {
                if selector.is_empty() {
                    println!("Usage: highlight <selector>");
                    continue;
                }
                match client.browser_highlight(&selector).await {
                    Ok(_) => println!("✓ Highlighted {} (3s)", selector),
                    Err(e) => println!("❌ Highlight failed: {}", e),
                }
            }
            ShellCommand::Find { selector } => {
                if selector.is_empty() {
                    println!("Usage: find <selector>");
                    continue;
                }
                match client.browser_find(&selector).await {
                    Ok(data) => {
                        let count = data["count"].as_u64().unwrap_or(0);
                        if count == 0 {
                            println!("  No elements found matching '{}'", selector);
                        } else {
                            println!("  Found {} element(s) matching '{}'", count, selector);
                            if let Some(matches) = data["matches"].as_array() {
                                for (i, m) in matches.iter().enumerate() {
                                    let tag = m["tag"].as_str().unwrap_or("?");
                                    let id = m["id"].as_str().filter(|s| !s.is_empty());
                                    let classes = m["classes"].as_str().filter(|s| !s.is_empty());
                                    let text = m["text"].as_str().unwrap_or("");
                                    let mut desc = format!("<{}", tag);
                                    if let Some(id) = id {
                                        desc.push_str(&format!(" id=\"{}\"", id));
                                    }
                                    if let Some(cls) = classes {
                                        desc.push_str(&format!(" class=\"{}\"", cls));
                                    }
                                    desc.push('>');
                                    if !text.is_empty() {
                                        desc.push_str(&format!(" \"{}\"", text));
                                    }
                                    println!("    [{}] {}", i, desc);
                                }
                                if count > 10 {
                                    println!("    ... and {} more", count - 10);
                                }
                            }
                        }
                    }
                    Err(e) => println!("❌ Find failed: {}", e),
                }
            }

            // --- Workflow commands ---
            ShellCommand::Run { path, params } => {
                let hash_params: HashMap<String, String> = params.into_iter().collect();
                match client.workflow_run(path.to_str().unwrap_or(""), hash_params).await {
                    Ok(result) => {
                        if result["success"].as_bool().unwrap_or(false) {
                            println!("✅ Workflow completed!");
                        } else {
                            println!(
                                "❌ Workflow failed: {}",
                                result["error"].as_str().unwrap_or("unknown error")
                            );
                        }
                        if let Some(ms) = result["duration_ms"].as_u64() {
                            println!("  Duration: {}ms", ms);
                        }
                    }
                    Err(e) => println!("❌ Error: {}", e),
                }
            }
            ShellCommand::Trace { path, params } => {
                println!("🔍 Running with trace logging...");
                let hash_params: HashMap<String, String> = params.into_iter().collect();
                match client.workflow_run(path.to_str().unwrap_or(""), hash_params).await {
                    Ok(_) => {}
                    Err(e) => println!("❌ Trace error: {}", e),
                }
            }
            ShellCommand::List => match client.workflow_list().await {
                Ok(workflows) => {
                    if workflows.is_empty() {
                        println!("  (no workflows found)");
                    } else {
                        for w in &workflows {
                            println!("  {}", w);
                        }
                    }
                }
                Err(e) => println!("❌ {}", e),
            },

            // --- Debug commands (no-op in daemon mode for now) ---
            ShellCommand::DebugOn { profile } => {
                if let Some(p) = &profile {
                    println!("✓ Debug mode ON (profile: {})", p);
                } else {
                    println!("✓ Debug mode ON");
                }
            }
            ShellCommand::DebugOff => {
                println!("✓ Debug mode OFF");
            }
            ShellCommand::DebugStatus => {
                println!("  Debug: off (toggle with 'debug on')");
                println!("  Mode: daemon-connected");
            }
            ShellCommand::DebugClean => {
                match client.debug_clean().await {
                    Ok(data) => {
                        let removed = data["files_removed"].as_u64().unwrap_or(0);
                        let freed = data["bytes_freed"].as_u64().unwrap_or(0);
                        let remaining = data["files_remaining"].as_u64().unwrap_or(0);
                        let freed_str = if freed >= 1_048_576 {
                            format!("{:.1} MB", freed as f64 / 1_048_576.0)
                        } else if freed >= 1024 {
                            format!("{:.1} KB", freed as f64 / 1024.0)
                        } else {
                            format!("{} bytes", freed)
                        };
                        println!("✓ Removed {} files ({}), {} remaining", removed, freed_str, remaining);
                    }
                    Err(e) => println!("❌ Cleanup failed: {}", e),
                }
            }
            ShellCommand::Tabs => {
                match client.browser_tab_list().await {
                    Ok(tabs) => {
                        println!("Open tabs ({}):", tabs.len());
                        for tab in &tabs {
                            let active = tab["active"].as_bool().unwrap_or(false);
                            let marker = if active { " *" } else { "  " };
                            let index = tab["index"].as_u64().unwrap_or(0);
                            let url = tab["url"].as_str().unwrap_or("about:blank");
                            println!("{} [{}] {}", marker, index, url);
                        }
                    }
                    Err(e) => println!("❌ Failed to list tabs: {}", e),
                }
            }
            ShellCommand::TabNew { url } => {
                match client.browser_tab_new(url.as_deref()).await {
                    Ok(index) => println!("✓ Opened new tab {}", index),
                    Err(e) => println!("❌ Failed to open tab: {}", e),
                }
            }
            ShellCommand::TabSwitch { index } => {
                match client.browser_tab_switch(index).await {
                    Ok(()) => println!("✓ Switched to tab {}", index),
                    Err(e) => println!("❌ Failed to switch tab: {}", e),
                }
            }
            ShellCommand::TabClose { index } => {
                let tab_index = if let Some(idx) = index {
                    idx
                } else {
                    // Close the current/active tab
                    match client.browser_tab_list().await {
                        Ok(tabs) => tabs.iter()
                            .find(|t| t["active"].as_bool().unwrap_or(false))
                            .and_then(|t| t["index"].as_u64())
                            .unwrap_or(0) as usize,
                        Err(_) => 0,
                    }
                };
                match client.browser_tab_close(tab_index).await {
                    Ok(()) => println!("✓ Closed tab {}", tab_index),
                    Err(e) => println!("❌ Failed to close tab: {}", e),
                }
            }
            ShellCommand::Pdf { path } => {
                match client.browser_pdf().await {
                    Ok(data) => {
                        let path = path.unwrap_or_else(|| {
                            PathBuf::from(format!(
                                "page_{}.pdf",
                                chrono::Utc::now().format("%Y%m%d_%H%M%S")
                            ))
                        });
                        match std::fs::write(&path, &data) {
                            Ok(_) => println!("✓ PDF saved to {}", path.display()),
                            Err(e) => println!("❌ Failed to save PDF: {}", e),
                        }
                    }
                    Err(e) => println!("❌ PDF export failed: {}", e),
                }
            }
            ShellCommand::Unknown { command } => {
                if !command.is_empty() {
                    println!(
                        "Unknown command: '{}'. Type 'help' for commands.",
                        command
                    );
                }
            }
        }
    }

    if let Err(e) = shell.save_history() {
        eprintln!("Warning: {}", e);
    }

    Ok(())
}

/// Format a chrono::Duration as human-readable string
fn format_duration(d: chrono::Duration) -> String {
    let secs = d.num_seconds();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
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
        .execute_with_pause_handler(&workflow, adapter, params, ResolvedDebugConfig::default(), &ShellPauseHandler, None)
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
