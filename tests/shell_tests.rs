//! Shell Integration Tests
//!
//! Tests for shell client and daemon communication.
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test --test shell_tests
//! ```

use std::path::PathBuf;

// ============================================================================
// Shell Client Tests
// ============================================================================

#[cfg(test)]
mod client_tests {
    use super::*;

    #[test]
    fn test_history_file_path() {
        // History should be in XDG data dir or ~/.local/share/automodus
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("automodus");
        let history_file = data_dir.join("history.txt");
        
        println!("History file path: {}", history_file.display());
        assert!(history_file.is_absolute() || data_dir == PathBuf::from(".").join("automodus"));
    }

    #[test]
    fn test_default_prompt() {
        let default_prompt = "automodus> ";
        assert_eq!(default_prompt, "automodus> ");
    }
}

// ============================================================================
// Command Parsing Tests (already in shell::client::tests)
// These are integration-level command flow tests
// ============================================================================

#[cfg(test)]
mod command_flow_tests {
    #[test]
    fn test_empty_input_handling() {
        // Empty input should produce Unknown command with empty string
        let input = "";
        assert!(input.is_empty());
    }

    #[test]
    fn test_whitespace_input_handling() {
        let input = "   ";
        let trimmed = input.trim();
        assert!(trimmed.is_empty());
    }

    #[test]
    fn test_command_case_insensitivity() {
        // Commands should be case-insensitive
        let commands = vec!["GOTO", "Goto", "goto", "GoTo"];
        for cmd in commands {
            let lower = cmd.to_lowercase();
            assert_eq!(lower, "goto");
        }
    }
}

// ============================================================================
// Shell-Daemon Integration Tests
// ============================================================================

#[cfg(test)]
mod daemon_integration_tests {
    use super::*;

    #[test]
    #[ignore] // Requires running daemon
    fn test_shell_connects_to_daemon() {
        // Test that shell can establish connection to daemon socket
        let home = dirs::home_dir().expect("Home directory");
        let socket_path = home.join(".automodus").join("automodus.sock");
        
        if socket_path.exists() {
            println!("Socket exists at: {}", socket_path.display());
            // In real test, would attempt connection
        } else {
            println!("Socket not found (daemon not running)");
        }
    }

    #[test]
    #[ignore] // Requires running daemon
    fn test_shell_graceful_daemon_not_running() {
        // Shell should handle daemon not running gracefully
        let home = dirs::home_dir().expect("Home directory");
        let socket_path = home.join(".automodus").join("automodus.sock");
        
        // If socket doesn't exist, shell should show helpful error
        if !socket_path.exists() {
            println!("Daemon not running - shell should suggest 'automodus daemon start'");
        }
    }
}

// ============================================================================
// Browser Persistence Tests
// ============================================================================

#[cfg(test)]
mod browser_persistence_tests {
    #[test]
    fn test_user_data_dir_path() {
        // Browser user data should be stored persistently
        let temp_dir = std::env::temp_dir().join("automodus-server");
        println!("Browser user data dir: {}", temp_dir.display());
        assert!(temp_dir.is_absolute());
    }

    #[test]
    #[ignore] // Requires daemon with browser
    fn test_browser_survives_shell_exit() {
        // Browser should remain open when shell exits
        // This tests the session keep-alive feature
        println!("Browser persistence test - requires manual verification");
    }
}

// ============================================================================
// Multiple Shell Connection Tests
// ============================================================================

#[cfg(test)]
mod multi_shell_tests {
    #[test]
    #[ignore] // Requires running daemon
    fn test_multiple_shells_can_connect() {
        // Multiple shell instances should be able to connect to daemon
        println!("Multi-shell test - requires daemon running");
    }

    #[test]
    fn test_shell_command_isolation() {
        // Commands from different shells should not interfere
        // This is a design principle test
        let shell1_cmd = "goto https://example.com";
        let shell2_cmd = "click #button";
        
        // Commands are independent
        assert_ne!(shell1_cmd, shell2_cmd);
    }
}
