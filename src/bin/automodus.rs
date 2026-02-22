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
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::debug;

use automodus::{
    actions::BrowserHandle,
    api,
    core::WorkflowEngine,
    modules::ChromePageAdapter,
    utils::{logging, yaml_to_json},
    workflow::{WorkflowLoader, WorkflowParser},
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

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
    Run { path: PathBuf, keep_open: bool },
    /// Start the API server
    Serve,
    /// Validate workflow files
    Validate { path: PathBuf },
    /// List all loaded workflows
    List,
    /// Interactive shell mode - keeps browser running
    Shell,
    /// Show help
    Help,
}

fn parse_args() -> Command {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        return Command::Help;
    }

    match args[1].as_str() {
        "run" => {
            if args.len() < 3 {
                eprintln!("Usage: automodus run <workflow.yaml> [--keep-open]");
                std::process::exit(1);
            }
            let keep_open = args.iter().any(|a| a == "--keep-open" || a == "-k");
            Command::Run {
                path: PathBuf::from(&args[2]),
                keep_open,
            }
        }
        "shell" => Command::Shell,
        "serve" => Command::Serve,
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
        -k, --keep-open Keep browser open after workflow completes
    shell               Interactive shell mode (keeps browser running)
    serve               Start the API server
    validate [path]     Validate workflow files (default: workflows/)
    list                List all loaded workflows
    help                Show this help message

EXAMPLES:
    # Run a specific workflow
    automodus run workflows/example.yaml

    # Run workflow and keep browser open
    automodus run workflows/login.yaml --keep-open

    # Start interactive shell for testing
    automodus shell

    # Validate all workflows in a directory
    automodus validate workflows/

ENVIRONMENT:
    AUTOMODUS_CONFIG    Path to config file (default: config/app.toml)
    AUTOMODUS_WORKFLOWS Path to workflows directory (default: workflows/)
    RUST_LOG            Logging level (default: info)

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
        Command::Run { path, keep_open } => {
            print_banner();
            run_workflow(&path, keep_open).await?;
        }
        Command::Shell => {
            print_banner();
            run_shell().await?;
        }
        Command::Serve => {
            print_banner();
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

async fn run_workflow(
    path: &std::path::Path,
    keep_open: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Loading workflow from: {}", path.display());

    // Parse workflow
    let content = std::fs::read_to_string(path)?;
    let workflow = WorkflowParser::parse(&content)?;

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
        .execute(&workflow, &adapter, params)
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
