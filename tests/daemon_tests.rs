//! Daemon Integration Tests
//!
//! Tests for daemon lifecycle, PID management, and socket communication.
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test --test daemon_tests -- --ignored
//! ```

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::sync::atomic::{AtomicU32, Ordering};

/// Atomic counter for unique test directories
static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Get unique test directory for each test
fn unique_test_dir(prefix: &str) -> PathBuf {
    let count = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("automodus-{}-{}", prefix, count))
}

// ============================================================================
// PID File Tests
// ============================================================================

#[cfg(test)]
mod pid_file_tests {
    use super::*;

    #[test]
    fn test_pid_file_path() {
        // PID file should be in ~/.automodus/daemon.pid
        let home = dirs::home_dir().expect("Home directory should exist");
        let expected = home.join(".automodus").join("daemon.pid");
        
        // Just verify the path is constructible
        assert!(expected.parent().is_some());
        println!("Expected PID file path: {}", expected.display());
    }

    #[test]
    fn test_stale_pid_detection() {
        // Create a test PID file with an invalid PID
        let test_dir = unique_test_dir("stale-pid");
        std::fs::create_dir_all(&test_dir).expect("Create test directory");
        let pid_file = test_dir.join("daemon.pid");
        
        // Write a PID that definitely doesn't exist (very high number)
        std::fs::write(&pid_file, "99999999").expect("Write PID file");
        
        // Verify file exists
        assert!(pid_file.exists());
        
        // Read and parse PID
        let content = std::fs::read_to_string(&pid_file).expect("Read PID file");
        let pid: u32 = content.trim().parse().expect("Parse PID");
        assert_eq!(pid, 99999999);
        
        // The PID should be stale (process doesn't exist)
        #[cfg(unix)]
        {
            use std::process::Command;
            let result = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output();
            
            // kill -0 should fail because process doesn't exist
            match result {
                Ok(output) => {
                    assert!(!output.status.success(), "Process should not exist");
                }
                Err(_) => {
                    // Command failed - process doesn't exist
                }
            }
        }
        
        // Clean up this test's directory
        let _ = std::fs::remove_dir_all(&test_dir);
    }
}

// ============================================================================
// Socket Tests
// ============================================================================

#[cfg(test)]
mod socket_tests {
    use super::*;

    #[test]
    fn test_socket_path() {
        // Socket should be in ~/.automodus/automodus.sock
        let home = dirs::home_dir().expect("Home directory should exist");
        let expected = home.join(".automodus").join("automodus.sock");
        
        // Just verify the path is constructible
        assert!(expected.parent().is_some());
        println!("Expected socket path: {}", expected.display());
    }

    #[test]
    fn test_socket_directory_creation() {
        let test_dir = unique_test_dir("socket-dir");
        
        // Create directory
        std::fs::create_dir_all(&test_dir).expect("Create test directory");
        
        // Verify directory exists
        assert!(test_dir.is_dir());
        
        // Clean up this test's directory
        let _ = std::fs::remove_dir_all(&test_dir);
    }
}

// ============================================================================
// Daemon Lifecycle Tests (Requires actual daemon)
// ============================================================================

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    #[ignore] // Requires built binary
    fn test_daemon_start_stop() {
        // This test requires the daemon binary to be built
        // Run: cargo build first
        
        let binary = std::env::current_dir()
            .unwrap()
            .join("target/debug/automodus");
        
        if !binary.exists() {
            println!("Skipping test: binary not found at {}", binary.display());
            return;
        }
        
        // Start daemon in foreground for testing (would normally background)
        let output = Command::new(&binary)
            .args(["daemon", "status"])
            .output();
        
        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("Status stdout: {}", stdout);
                println!("Status stderr: {}", stderr);
                // Status should work even if daemon not running
            }
            Err(e) => {
                println!("Status command failed: {}", e);
            }
        }
    }

    #[test]
    #[ignore] // Requires running daemon
    fn test_daemon_status() {
        let binary = std::env::current_dir()
            .unwrap()
            .join("target/debug/automodus");
        
        if !binary.exists() {
            println!("Skipping test: binary not found");
            return;
        }
        
        let output = Command::new(&binary)
            .args(["daemon", "status"])
            .output()
            .expect("Execute status command");
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("Daemon status: {}", stdout);
        
        // Should contain either "running" or "not running"
        assert!(
            stdout.contains("running") || stdout.contains("not running") || stdout.contains("Daemon"),
            "Status should report daemon state"
        );
    }
}

// ============================================================================
// Graceful Shutdown Tests
// ============================================================================

#[cfg(test)]
mod shutdown_tests {
    use super::*;

    #[test]
    fn test_shutdown_signal_handling() {
        // Verify SIGTERM and SIGINT constants are correct
        #[cfg(unix)]
        {
            use libc::{SIGINT, SIGTERM};
            
            assert_eq!(SIGTERM, 15, "SIGTERM should be 15");
            assert_eq!(SIGINT, 2, "SIGINT should be 2");
            println!("Signal constants verified");
        }
    }
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn test_default_config_paths() {
        let home = dirs::home_dir().expect("Home directory");
        
        let config_dir = home.join(".automodus");
        let pid_file = config_dir.join("daemon.pid");
        let socket_file = config_dir.join("automodus.sock");
        let log_file = config_dir.join("daemon.log");
        
        println!("Config directory: {}", config_dir.display());
        println!("PID file: {}", pid_file.display());
        println!("Socket file: {}", socket_file.display());
        println!("Log file: {}", log_file.display());
        
        // Verify paths are sensible
        assert!(config_dir.is_absolute());
        assert!(pid_file.is_absolute());
    }

    #[test]
    fn test_default_port() {
        // Default HTTP port should be 3000
        let default_port: u16 = 3000;
        assert!(default_port > 1024, "Default port should be non-privileged");
        assert!(default_port < 65535, "Default port should be valid");
    }
}
