//! Workflow Metrics and Observability
//!
//! Data structures and utilities for tracking workflow execution metrics.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

/// Metrics tracking for workflow execution
#[derive(Debug, Clone, Default)]
pub struct WorkflowMetrics {
    /// Total workflows executed
    pub workflows_executed: Arc<AtomicU64>,
    /// Total workflows succeeded
    pub workflows_succeeded: Arc<AtomicU64>,
    /// Total workflows failed
    pub workflows_failed: Arc<AtomicU64>,
    /// Total steps executed
    pub steps_executed: Arc<AtomicU64>,
    /// Total action errors
    pub action_errors: Arc<AtomicU64>,
    /// Last activity timestamp
    pub last_activity: Arc<AtomicU64>,
}

/// Snapshot of workflow metrics for serialization
#[derive(Debug, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub workflows_executed: u64,
    pub workflows_succeeded: u64,
    pub workflows_failed: u64,
    pub steps_executed: u64,
    pub action_errors: u64,
    pub success_rate: f64,
    pub last_activity: Option<u64>,
}

impl WorkflowMetrics {
    /// Create new workflow metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a workflow execution start
    pub fn record_workflow_started(&self) {
        self.workflows_executed.fetch_add(1, Ordering::Relaxed);
        self.update_last_activity();
    }

    /// Record a workflow success
    pub fn record_workflow_success(&self) {
        self.workflows_succeeded.fetch_add(1, Ordering::Relaxed);
        self.update_last_activity();
    }

    /// Record a workflow failure
    pub fn record_workflow_failure(&self) {
        self.workflows_failed.fetch_add(1, Ordering::Relaxed);
        self.update_last_activity();
    }

    /// Record step execution
    pub fn record_step_executed(&self) {
        self.steps_executed.fetch_add(1, Ordering::Relaxed);
        self.update_last_activity();
    }

    /// Record an action error
    pub fn record_action_error(&self) {
        self.action_errors.fetch_add(1, Ordering::Relaxed);
        self.update_last_activity();
    }

    fn update_last_activity(&self) {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.last_activity.store(now, Ordering::Relaxed);
    }

    /// Get a snapshot of current metrics
    pub fn snapshot(&self) -> MetricsSnapshot {
        let last_activity = self.last_activity.load(Ordering::Relaxed);
        let executed = self.workflows_executed.load(Ordering::Relaxed);
        let succeeded = self.workflows_succeeded.load(Ordering::Relaxed);

        let success_rate = if executed > 0 {
            succeeded as f64 / executed as f64 * 100.0
        } else {
            0.0
        };

        MetricsSnapshot {
            workflows_executed: executed,
            workflows_succeeded: succeeded,
            workflows_failed: self.workflows_failed.load(Ordering::Relaxed),
            steps_executed: self.steps_executed.load(Ordering::Relaxed),
            action_errors: self.action_errors.load(Ordering::Relaxed),
            success_rate,
            last_activity: if last_activity == 0 {
                None
            } else {
                Some(last_activity)
            },
        }
    }

    /// Reset all metrics
    pub fn reset(&self) {
        self.workflows_executed.store(0, Ordering::Relaxed);
        self.workflows_succeeded.store(0, Ordering::Relaxed);
        self.workflows_failed.store(0, Ordering::Relaxed);
        self.steps_executed.store(0, Ordering::Relaxed);
        self.action_errors.store(0, Ordering::Relaxed);
        self.last_activity.store(0, Ordering::Relaxed);
    }

    /// Get last activity as DateTime<Utc>, or None if never active
    pub fn last_activity(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let ts = self.last_activity.load(Ordering::Relaxed);
        if ts == 0 {
            None
        } else {
            chrono::DateTime::from_timestamp(ts as i64, 0)
        }
    }
}
