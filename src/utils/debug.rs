//! Debug Directory Cleanup
//!
//! Manages cleanup of debug output files (screenshots, traces) that
//! accumulate during workflow execution.

use std::path::Path;
use std::time::{Duration, SystemTime};
use std::{fs, io};

use tracing::{debug, info, warn};

/// Policy controlling which debug files to remove.
#[derive(Debug, Clone)]
pub struct CleanupPolicy {
    /// Remove files older than this duration.
    pub max_age: Option<Duration>,
    /// Keep at most this many files (newest first).
    pub max_files: Option<usize>,
    /// Keep total directory size under this limit in bytes.
    pub max_size_bytes: Option<u64>,
}

impl Default for CleanupPolicy {
    fn default() -> Self {
        Self {
            max_age: Some(Duration::from_secs(7 * 24 * 3600)), // 7 days
            max_files: Some(100),
            max_size_bytes: None,
        }
    }
}

/// Result of a cleanup operation.
#[derive(Debug, Clone, Default)]
pub struct CleanupStats {
    /// Number of files removed.
    pub files_removed: usize,
    /// Total bytes freed.
    pub bytes_freed: u64,
    /// Number of files remaining.
    pub files_remaining: usize,
}

impl std::fmt::Display for CleanupStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let freed = if self.bytes_freed >= 1_048_576 {
            format!("{:.1} MB", self.bytes_freed as f64 / 1_048_576.0)
        } else if self.bytes_freed >= 1024 {
            format!("{:.1} KB", self.bytes_freed as f64 / 1024.0)
        } else {
            format!("{} bytes", self.bytes_freed)
        };
        write!(
            f,
            "Removed {} files ({}), {} remaining",
            self.files_removed, freed, self.files_remaining
        )
    }
}

/// Entry representing a file in the debug directory.
struct FileEntry {
    path: std::path::PathBuf,
    modified: SystemTime,
    size: u64,
}

/// Clean up debug output files in the given directory according to policy.
///
/// Files are evaluated in order: max_age first, then max_files, then max_size.
/// Only regular files are considered; subdirectories are skipped.
pub fn cleanup_debug_dir(dir: &Path, policy: &CleanupPolicy) -> io::Result<CleanupStats> {
    if !dir.exists() {
        return Ok(CleanupStats::default());
    }

    let mut entries = collect_file_entries(dir)?;
    let mut stats = CleanupStats::default();
    let now = SystemTime::now();

    // Sort newest first (most recent modification time first)
    entries.sort_by(|a, b| b.modified.cmp(&a.modified));

    // Track which files to remove (by index)
    let mut to_remove = vec![false; entries.len()];

    // Phase 1: Remove files older than max_age
    if let Some(max_age) = policy.max_age {
        for (i, entry) in entries.iter().enumerate() {
            if let Ok(age) = now.duration_since(entry.modified) {
                if age > max_age {
                    to_remove[i] = true;
                }
            }
        }
    }

    // Phase 2: Keep at most max_files (newest first, only count non-removed)
    if let Some(max_files) = policy.max_files {
        let mut kept = 0;
        for (i, _entry) in entries.iter().enumerate() {
            if to_remove[i] {
                continue;
            }
            kept += 1;
            if kept > max_files {
                to_remove[i] = true;
            }
        }
    }

    // Phase 3: Enforce max total size (newest first, only count non-removed)
    if let Some(max_size) = policy.max_size_bytes {
        let mut total = 0u64;
        for (i, entry) in entries.iter().enumerate() {
            if to_remove[i] {
                continue;
            }
            total = total.saturating_add(entry.size);
            if total > max_size {
                to_remove[i] = true;
            }
        }
    }

    // Execute removals
    for (i, entry) in entries.iter().enumerate() {
        if to_remove[i] {
            match fs::remove_file(&entry.path) {
                Ok(()) => {
                    debug!(path = %entry.path.display(), "Removed debug file");
                    stats.files_removed += 1;
                    stats.bytes_freed += entry.size;
                }
                Err(e) => {
                    warn!(path = %entry.path.display(), error = %e, "Failed to remove debug file");
                }
            }
        }
    }

    stats.files_remaining = entries.len() - stats.files_removed;

    info!(
        dir = %dir.display(),
        removed = stats.files_removed,
        freed = stats.bytes_freed,
        remaining = stats.files_remaining,
        "Debug directory cleanup complete"
    );

    Ok(stats)
}

/// List debug files in a directory with their metadata.
pub fn list_debug_files(dir: &Path) -> io::Result<Vec<(String, u64, SystemTime)>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let entries = collect_file_entries(dir)?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let name = e
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            (name, e.size, e.modified)
        })
        .collect())
}

fn collect_file_entries(dir: &Path) -> io::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if !metadata.is_file() {
            continue;
        }

        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        entries.push(FileEntry {
            path: entry.path(),
            modified,
            size: metadata.len(),
        });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_test_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_cleanup_empty_dir() {
        let dir = create_test_dir();
        let stats = cleanup_debug_dir(dir.path(), &CleanupPolicy::default()).unwrap();
        assert_eq!(stats.files_removed, 0);
        assert_eq!(stats.files_remaining, 0);
    }

    #[test]
    fn test_cleanup_nonexistent_dir() {
        let stats = cleanup_debug_dir(Path::new("/tmp/nonexistent_automodus_test"), &CleanupPolicy::default()).unwrap();
        assert_eq!(stats.files_removed, 0);
    }

    #[test]
    fn test_cleanup_max_files() {
        let dir = create_test_dir();
        for i in 0..10 {
            fs::write(dir.path().join(format!("file_{}.png", i)), vec![0u8; 100]).unwrap();
            // Small delay to ensure different modification times
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let policy = CleanupPolicy {
            max_age: None,
            max_files: Some(5),
            max_size_bytes: None,
        };

        let stats = cleanup_debug_dir(dir.path(), &policy).unwrap();
        assert_eq!(stats.files_removed, 5);
        assert_eq!(stats.files_remaining, 5);
    }

    #[test]
    fn test_cleanup_max_size() {
        let dir = create_test_dir();
        for i in 0..5 {
            fs::write(dir.path().join(format!("file_{}.png", i)), vec![0u8; 1000]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let policy = CleanupPolicy {
            max_age: None,
            max_files: None,
            max_size_bytes: Some(2500), // Should keep ~2 files
        };

        let stats = cleanup_debug_dir(dir.path(), &policy).unwrap();
        assert!(stats.files_removed >= 2);
        assert!(stats.files_remaining <= 3);
    }

    #[test]
    fn test_cleanup_stats_display() {
        let stats = CleanupStats {
            files_removed: 5,
            bytes_freed: 2_500_000,
            files_remaining: 10,
        };
        let display = format!("{}", stats);
        assert!(display.contains("5 files"));
        assert!(display.contains("MB"));
        assert!(display.contains("10 remaining"));
    }
}
