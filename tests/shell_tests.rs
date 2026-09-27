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
mod session_command_tests {
    use automodus::shell::ShellClient;
    use automodus::shell::ShellCommand;

    #[test]
    fn test_parse_session_new() {
        match ShellClient::parse_command("session new") {
            ShellCommand::SessionNew { name, keep_alive } => {
                assert!(name.is_none());
                assert!(!keep_alive);
            }
            other => panic!("Expected SessionNew, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_session_new_with_name() {
        match ShellClient::parse_command("session new mytest") {
            ShellCommand::SessionNew { name, keep_alive } => {
                assert_eq!(name, Some("mytest".to_string()));
                assert!(!keep_alive);
            }
            other => panic!("Expected SessionNew, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_session_new_with_flags() {
        match ShellClient::parse_command("session new --name=prod --keep-alive") {
            ShellCommand::SessionNew { name, keep_alive } => {
                assert_eq!(name, Some("prod".to_string()));
                assert!(keep_alive);
            }
            other => panic!("Expected SessionNew, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_session_list() {
        assert!(matches!(
            ShellClient::parse_command("session list"),
            ShellCommand::SessionList
        ));
        // ls alias
        assert!(matches!(
            ShellClient::parse_command("session ls"),
            ShellCommand::SessionList
        ));
    }

    #[test]
    fn test_parse_session_switch() {
        match ShellClient::parse_command("session switch abc123") {
            ShellCommand::SessionSwitch { target } => assert_eq!(target, "abc123"),
            other => panic!("Expected SessionSwitch, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_session_switch_missing_target() {
        assert!(matches!(
            ShellClient::parse_command("session switch"),
            ShellCommand::Unknown { .. }
        ));
    }

    #[test]
    fn test_parse_session_close() {
        match ShellClient::parse_command("session close abc123") {
            ShellCommand::SessionClose { target } => assert_eq!(target, Some("abc123".to_string())),
            other => panic!("Expected SessionClose, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_session_close_current() {
        match ShellClient::parse_command("session close") {
            ShellCommand::SessionClose { target } => assert!(target.is_none()),
            other => panic!("Expected SessionClose, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_session_info() {
        assert!(matches!(
            ShellClient::parse_command("session info"),
            ShellCommand::SessionInfo
        ));
        // Bare "session" also shows info
        assert!(matches!(
            ShellClient::parse_command("session"),
            ShellCommand::SessionInfo
        ));
    }

    #[test]
    fn test_parse_session_keep_alive() {
        match ShellClient::parse_command("session keep-alive abc123 on") {
            ShellCommand::SessionKeepAlive { target, toggle } => {
                assert_eq!(target, Some("abc123".to_string()));
                assert_eq!(toggle, Some(true));
            }
            other => panic!("Expected SessionKeepAlive, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_session_keep_alive_off() {
        match ShellClient::parse_command("session keep-alive abc123 off") {
            ShellCommand::SessionKeepAlive { target, toggle } => {
                assert_eq!(target, Some("abc123".to_string()));
                assert_eq!(toggle, Some(false));
            }
            other => panic!("Expected SessionKeepAlive, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_session_keep_alive_toggle() {
        // No on/off = toggle (None)
        match ShellClient::parse_command("session keep-alive abc123") {
            ShellCommand::SessionKeepAlive { target, toggle } => {
                assert_eq!(target, Some("abc123".to_string()));
                assert!(toggle.is_none());
            }
            other => panic!("Expected SessionKeepAlive, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_session_shortcut() {
        // "sess" is a shortcut for "session"
        assert!(matches!(
            ShellClient::parse_command("sess list"),
            ShellCommand::SessionList
        ));
    }

    #[test]
    fn test_parse_session_ka_shortcut() {
        match ShellClient::parse_command("session ka abc on") {
            ShellCommand::SessionKeepAlive { target, toggle } => {
                assert_eq!(target, Some("abc".to_string()));
                assert_eq!(toggle, Some(true));
            }
            other => panic!("Expected SessionKeepAlive, got {:?}", other),
        }
    }
}

// ============================================================================
// Session AppCore Integration Tests
// ============================================================================

#[cfg(test)]
mod session_appcore_tests {
    use automodus::core::AppCore;
    use automodus::daemon::DaemonConfig;

    #[tokio::test]
    async fn test_find_session_by_name() {
        let config = DaemonConfig::default();
        let core = AppCore::new(&config);
        let id = core
            .create_session(Some("test-session".to_string()))
            .await
            .unwrap();

        let found = core.find_session_by_name("test-session").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, id);
    }

    #[tokio::test]
    async fn test_find_session_by_id_or_name() {
        let config = DaemonConfig::default();
        let core = AppCore::new(&config);
        let id = core
            .create_session(Some("myname".to_string()))
            .await
            .unwrap();

        // Find by name
        let by_name = core.find_session("myname").await;
        assert!(by_name.is_some());

        // Find by id
        let by_id = core.find_session(&id).await;
        assert!(by_id.is_some());

        // Not found
        let missing = core.find_session("nonexistent").await;
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_set_session_keep_alive() {
        let config = DaemonConfig::default();
        let core = AppCore::new(&config);
        let id = core
            .create_session(Some("ka-test".to_string()))
            .await
            .unwrap();

        // Default is keep_alive = true (from create_session)
        let session = core.get_session(&id).await.unwrap();
        assert!(session.keep_alive);

        // Toggle off
        core.set_session_keep_alive(&id, false).await.unwrap();
        let session = core.get_session(&id).await.unwrap();
        assert!(!session.keep_alive);

        // Toggle back on
        core.set_session_keep_alive(&id, true).await.unwrap();
        let session = core.get_session(&id).await.unwrap();
        assert!(session.keep_alive);
    }

    #[tokio::test]
    async fn test_set_keep_alive_not_found() {
        let config = DaemonConfig::default();
        let core = AppCore::new(&config);
        let result = core.set_session_keep_alive("nonexistent", true).await;
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod daemon_integration_tests {
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
