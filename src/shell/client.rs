//! Shell Client
//!
//! Interactive shell with readline support via rustyline.
//! Provides command history, line editing, and completion.

use std::collections::HashMap;
use std::path::PathBuf;

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Config, Context, Editor};

/// Shell configuration
#[derive(Debug, Clone)]
pub struct ShellConfig {
    /// History file path
    pub history_file: PathBuf,
    /// Maximum history size
    pub history_size: usize,
    /// Prompt string
    pub prompt: String,
    /// Available workflow directories
    pub workflow_dirs: Vec<PathBuf>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("automodus");

        Self {
            history_file: data_dir.join("history.txt"),
            history_size: 1000,
            prompt: "automodus> ".to_string(),
            workflow_dirs: vec![PathBuf::from("workflows")],
        }
    }
}

/// Parsed shell command
#[derive(Debug, Clone)]
pub enum ShellCommand {
    /// Run a workflow file
    Run {
        path: PathBuf,
        params: HashMap<String, String>,
    },
    /// Navigate to URL
    Goto { url: String },
    /// Click an element
    Click { selector: String },
    /// Type text into an element
    Type { selector: String, text: String },
    /// Wait for an element
    Wait { selector: String, timeout: Option<u64> },
    /// Take a screenshot
    Screenshot { path: Option<PathBuf> },
    /// Get element text
    Text { selector: String },
    /// Execute JavaScript
    Eval { script: String },
    /// List workflows
    List,
    /// Show current page status
    Status,
    /// Navigate back
    Back,
    /// Navigate forward
    Forward,
    /// Refresh page
    Refresh,
    /// Show help
    Help,
    /// Quit shell
    Quit,
    /// Debug on with optional profile
    DebugOn { profile: Option<String> },
    /// Debug off
    DebugOff,
    /// Debug status
    DebugStatus,
    /// Highlight an element
    Highlight { selector: String },
    /// Trace workflow (run with --debug=trace)
    Trace { path: PathBuf, params: HashMap<String, String> },
    /// Unknown command
    Unknown { command: String },
}

/// Shell command completer
#[derive(Clone)]
struct ShellCompleter {
    commands: Vec<String>,
    workflow_dirs: Vec<PathBuf>,
}

impl ShellCompleter {
    fn new(workflow_dirs: Vec<PathBuf>) -> Self {
        Self {
            commands: vec![
                "run".to_string(),
                "goto".to_string(),
                "click".to_string(),
                "type".to_string(),
                "wait".to_string(),
                "screenshot".to_string(),
                "text".to_string(),
                "eval".to_string(),
                "list".to_string(),
                "status".to_string(),
                "back".to_string(),
                "forward".to_string(),
                "refresh".to_string(),
                "debug".to_string(),
                "highlight".to_string(),
                "trace".to_string(),
                "help".to_string(),
                "quit".to_string(),
                "exit".to_string(),
            ],
            workflow_dirs,
        }
    }

    fn complete_command(&self, line: &str) -> Vec<Pair> {
        self.commands
            .iter()
            .filter(|cmd| cmd.starts_with(line))
            .map(|cmd| Pair {
                display: cmd.clone(),
                replacement: cmd.clone(),
            })
            .collect()
    }

    fn complete_workflow(&self, partial: &str) -> Vec<Pair> {
        let mut completions = Vec::new();

        for dir in &self.workflow_dirs {
            if let Ok(entries) = glob::glob(&format!("{}/**/*.yaml", dir.display())) {
                for entry in entries.flatten() {
                    let path_str = entry.display().to_string();
                    if path_str.contains(partial) || partial.is_empty() {
                        completions.push(Pair {
                            display: path_str.clone(),
                            replacement: path_str,
                        });
                    }
                }
            }
        }

        completions
    }
}

impl Completer for ShellCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let line = &line[..pos];
        let parts: Vec<&str> = line.split_whitespace().collect();

        match parts.len() {
            0 => {
                // Complete commands
                Ok((0, self.complete_command("")))
            }
            1 => {
                // Complete partial command
                if line.ends_with(' ') {
                    // Command complete, suggest arguments
                    match parts[0] {
                        "run" | "r" => Ok((pos, self.complete_workflow(""))),
                        _ => Ok((pos, vec![])),
                    }
                } else {
                    // Partial command
                    Ok((0, self.complete_command(parts[0])))
                }
            }
            _ => {
                // Complete arguments
                let cmd = parts[0];
                let last = parts.last().unwrap_or(&"");

                match cmd {
                    "run" | "r" => {
                        let start = line.rfind(' ').map(|i| i + 1).unwrap_or(0);
                        Ok((start, self.complete_workflow(last)))
                    }
                    _ => Ok((pos, vec![])),
                }
            }
        }
    }
}

impl Hinter for ShellCompleter {
    type Hint = String;

    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<Self::Hint> {
        None
    }
}

impl Highlighter for ShellCompleter {}
impl Validator for ShellCompleter {}
impl rustyline::Helper for ShellCompleter {}

/// Interactive shell client
pub struct ShellClient {
    /// Readline editor
    editor: Editor<ShellCompleter, DefaultHistory>,
    /// Shell configuration
    config: ShellConfig,
}

impl ShellClient {
    /// Create a new shell client
    pub fn new(config: ShellConfig) -> Result<Self, String> {
        // Create history directory
        if let Some(parent) = config.history_file.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        // Configure rustyline
        let rl_config = Config::builder()
            .history_ignore_dups(true)
            .map_err(|e| format!("Config error: {}", e))?
            .history_ignore_space(true)
            .max_history_size(config.history_size)
            .map_err(|e| format!("Config error: {}", e))?
            .auto_add_history(true)
            .build();

        let completer = ShellCompleter::new(config.workflow_dirs.clone());

        let mut editor = Editor::with_config(rl_config)
            .map_err(|e| format!("Failed to create editor: {}", e))?;

        editor.set_helper(Some(completer));

        // Load history
        if config.history_file.exists() {
            if let Err(e) = editor.load_history(&config.history_file) {
                eprintln!("Warning: Failed to load history: {}", e);
            }
        }

        Ok(Self { editor, config })
    }

    /// Read a line with prompt
    pub fn readline(&mut self) -> Result<String, ReadlineError> {
        self.editor.readline(&self.config.prompt)
    }

    /// Save history
    pub fn save_history(&mut self) -> Result<(), String> {
        self.editor
            .save_history(&self.config.history_file)
            .map_err(|e| format!("Failed to save history: {}", e))
    }

    /// Parse a command line into a ShellCommand
    pub fn parse_command(line: &str) -> ShellCommand {
        let line = line.trim();
        if line.is_empty() {
            return ShellCommand::Unknown {
                command: String::new(),
            };
        }

        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        let cmd = parts[0];
        let args = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match cmd.to_lowercase().as_str() {
            "run" | "r" => Self::parse_run_command(args),
            "goto" | "g" | "go" => ShellCommand::Goto {
                url: args.to_string(),
            },
            "click" | "c" => ShellCommand::Click {
                selector: args.to_string(),
            },
            "type" | "t" => Self::parse_type_command(args),
            "wait" | "w" => Self::parse_wait_command(args),
            "screenshot" | "ss" => ShellCommand::Screenshot {
                path: if args.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(args))
                },
            },
            "text" => ShellCommand::Text {
                selector: args.to_string(),
            },
            "eval" | "js" => ShellCommand::Eval {
                script: args.to_string(),
            },
            "list" | "ls" => ShellCommand::List,
            "status" | "s" => ShellCommand::Status,
            "back" => ShellCommand::Back,
            "forward" => ShellCommand::Forward,
            "refresh" | "reload" => ShellCommand::Refresh,
            "debug" => Self::parse_debug_command(args),
            "highlight" | "hl" => ShellCommand::Highlight {
                selector: args.to_string(),
            },
            "trace" => Self::parse_trace_command(args),
            "help" | "h" | "?" => ShellCommand::Help,
            "quit" | "exit" | "q" => ShellCommand::Quit,
            _ => ShellCommand::Unknown {
                command: cmd.to_string(),
            },
        }
    }

    fn parse_run_command(args: &str) -> ShellCommand {
        // Parse: path [key=value ...]
        let mut params = HashMap::new();
        let mut path: Option<PathBuf> = None;

        for part in Self::tokenize_args(args) {
            if let Some((key, value)) = part.split_once('=') {
                // Strip quotes from value
                let value = value
                    .trim_start_matches('"')
                    .trim_end_matches('"')
                    .trim_start_matches('\'')
                    .trim_end_matches('\'');
                params.insert(key.to_string(), value.to_string());
            } else if path.is_none() {
                path = Some(PathBuf::from(part));
            }
        }

        match path {
            Some(p) => ShellCommand::Run { path: p, params },
            None => ShellCommand::Unknown {
                command: "run".to_string(),
            },
        }
    }

    fn parse_type_command(args: &str) -> ShellCommand {
        // Parse: selector "text" or selector text
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return ShellCommand::Unknown {
                command: "type".to_string(),
            };
        }

        let selector = parts[0].to_string();
        let text = parts[1]
            .trim_start_matches('"')
            .trim_end_matches('"')
            .trim_start_matches('\'')
            .trim_end_matches('\'')
            .to_string();

        ShellCommand::Type { selector, text }
    }

    fn parse_wait_command(args: &str) -> ShellCommand {
        // Parse: selector [timeout_ms]
        let parts: Vec<&str> = args.split_whitespace().collect();
        if parts.is_empty() {
            return ShellCommand::Unknown {
                command: "wait".to_string(),
            };
        }

        let selector = parts[0].to_string();
        let timeout = parts.get(1).and_then(|s| s.parse().ok());

        ShellCommand::Wait { selector, timeout }
    }

    fn parse_debug_command(args: &str) -> ShellCommand {
        let parts: Vec<&str> = args.split_whitespace().collect();
        
        if parts.is_empty() {
            return ShellCommand::DebugStatus;
        }

        match parts[0] {
            "on" => {
                // Parse optional --profile=NAME
                let profile = parts.iter().find_map(|p| {
                    p.strip_prefix("--profile=")
                        .map(|s| s.to_string())
                });
                ShellCommand::DebugOn { profile }
            }
            "off" => ShellCommand::DebugOff,
            "status" => ShellCommand::DebugStatus,
            _ => ShellCommand::Unknown {
                command: format!("debug {}", args),
            },
        }
    }

    fn parse_trace_command(args: &str) -> ShellCommand {
        // Parse same as run but adds trace mode
        match Self::parse_run_command(args) {
            ShellCommand::Run { path, params } => ShellCommand::Trace { path, params },
            _ => ShellCommand::Unknown {
                command: "trace".to_string(),
            },
        }
    }

    /// Tokenize arguments handling quoted strings
    fn tokenize_args(input: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut quote_char = ' ';

        for ch in input.chars() {
            match ch {
                '"' | '\'' if !in_quotes => {
                    in_quotes = true;
                    quote_char = ch;
                }
                c if c == quote_char && in_quotes => {
                    in_quotes = false;
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

    /// Print help message
    pub fn print_help() {
        println!(
            r#"
Shell Commands:

  Navigation:
    goto <url>           Navigate to URL
    back                 Navigate back
    forward              Navigate forward
    refresh              Refresh page

  Interaction:
    click <selector>     Click an element
    type <sel> <text>    Type text into element
    wait <sel> [ms]      Wait for element

  Inspection:
    text <selector>      Get element text
    screenshot [path]    Take screenshot
    eval <js>            Execute JavaScript
    status               Show page URL

  Workflows:  
    run <file> [k=v...]  Run workflow file
    list                 List workflows

  Debug:
    debug on [--profile=NAME]  Enable debug mode
    debug off                   Disable debug mode
    debug status                Show debug status
    highlight <selector>        Highlight an element
    trace <file>                Run workflow with trace logging

  General:
    help                 Show this help
    quit                 Exit shell

Shortcuts: r=run, g=goto, c=click, t=type, w=wait, s=status, ls=list, hl=highlight, q=quit
"#
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_goto() {
        match ShellClient::parse_command("goto https://example.com") {
            ShellCommand::Goto { url } => assert_eq!(url, "https://example.com"),
            _ => panic!("Expected Goto"),
        }
    }

    #[test]
    fn test_parse_click() {
        match ShellClient::parse_command("click #submit") {
            ShellCommand::Click { selector } => assert_eq!(selector, "#submit"),
            _ => panic!("Expected Click"),
        }
    }

    #[test]
    fn test_parse_type() {
        match ShellClient::parse_command("type #input \"hello world\"") {
            ShellCommand::Type { selector, text } => {
                assert_eq!(selector, "#input");
                assert_eq!(text, "hello world");
            }
            _ => panic!("Expected Type"),
        }
    }

    #[test]
    fn test_parse_wait_with_timeout() {
        match ShellClient::parse_command("wait .element 5000") {
            ShellCommand::Wait { selector, timeout } => {
                assert_eq!(selector, ".element");
                assert_eq!(timeout, Some(5000));
            }
            _ => panic!("Expected Wait"),
        }
    }

    #[test]
    fn test_parse_run_with_params() {
        match ShellClient::parse_command("run test.yaml key=value name=\"John Doe\"") {
            ShellCommand::Run { path, params } => {
                assert_eq!(path, PathBuf::from("test.yaml"));
                assert_eq!(params.get("key"), Some(&"value".to_string()));
                assert_eq!(params.get("name"), Some(&"John Doe".to_string()));
            }
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_shortcut_commands() {
        assert!(matches!(
            ShellClient::parse_command("g https://example.com"),
            ShellCommand::Goto { .. }
        ));
        assert!(matches!(
            ShellClient::parse_command("c #btn"),
            ShellCommand::Click { .. }
        ));
        assert!(matches!(
            ShellClient::parse_command("q"),
            ShellCommand::Quit
        ));
    }

    #[test]
    fn test_parse_debug_on() {
        match ShellClient::parse_command("debug on") {
            ShellCommand::DebugOn { profile } => assert!(profile.is_none()),
            _ => panic!("Expected DebugOn"),
        }
    }

    #[test]
    fn test_parse_debug_on_with_profile() {
        match ShellClient::parse_command("debug on --profile=verbose") {
            ShellCommand::DebugOn { profile } => assert_eq!(profile, Some("verbose".to_string())),
            _ => panic!("Expected DebugOn with profile"),
        }
    }

    #[test]
    fn test_parse_debug_off() {
        assert!(matches!(
            ShellClient::parse_command("debug off"),
            ShellCommand::DebugOff
        ));
    }

    #[test]
    fn test_parse_highlight() {
        match ShellClient::parse_command("highlight #element") {
            ShellCommand::Highlight { selector } => assert_eq!(selector, "#element"),
            _ => panic!("Expected Highlight"),
        }
    }

    #[test]
    fn test_parse_trace() {
        match ShellClient::parse_command("trace workflow.yaml key=value") {
            ShellCommand::Trace { path, params } => {
                assert_eq!(path, PathBuf::from("workflow.yaml"));
                assert_eq!(params.get("key"), Some(&"value".to_string()));
            }
            _ => panic!("Expected Trace"),
        }
    }
}
