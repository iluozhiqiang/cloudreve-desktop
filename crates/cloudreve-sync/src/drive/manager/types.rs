use crate::drive::mounts::DriveConfig;
use crate::inventory::TaskRecord;
use crate::tasks::TaskProgress;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DriveState {
    pub drives: Vec<DriveConfig>,
}

impl DriveState {
    /// Read `drives.json` from disk (same format as [`crate::drive::manager::DriveManager::load`]).
    pub fn read_from_path(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read drive config file {}",
                path.display()
            )
        })?;
        serde_json::from_str(&content).context("Failed to parse drive config")
    }

    /// Write `drives.json` to disk (same formatting as [`crate::drive::manager::DriveManager::persist`]).
    pub fn write_to_path(&self, path: &Path) -> Result<()> {
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize drive state")?;
        fs::write(path, content).with_context(|| {
            format!(
                "Failed to write drive config file {}",
                path.display()
            )
        })
    }
}

/// Summary of the current status including drives and recent tasks
#[derive(Debug, Clone, Serialize)]
pub struct StatusSummary {
    /// All configured drives (unfiltered)
    pub drives: Vec<DriveConfig>,
    /// Active tasks (pending/running) with optional live progress info
    pub active_tasks: Vec<TaskWithProgress>,
    /// Recently finished tasks (completed/failed/cancelled)
    pub finished_tasks: Vec<TaskRecord>,
}

/// A task record with optional live progress information
#[derive(Debug, Clone, Serialize)]
pub struct TaskWithProgress {
    /// The task record from the database
    #[serde(flatten)]
    pub task: TaskRecord,
    /// Live progress information for running tasks (None if task is not currently running)
    pub live_progress: Option<TaskProgress>,
}

/// Capacity summary for UI display
#[derive(Debug, Clone, Serialize)]
pub struct CapacitySummary {
    pub total: i64,
    pub used: i64,
    pub label: String,
}

/// Sync status for UI display
#[derive(Debug, Clone, Serialize)]
pub enum SyncStatus {
    InSync,
    Syncing,
    Paused,
    Error,
}

/// Drive status information for external desktop surfaces
#[derive(Debug, Clone, Serialize)]
pub struct DriveStatusUI {
    pub name: String,
    pub raw_icon_path: Option<String>,
    pub capacity: Option<CapacitySummary>,
    pub profile_url: String,
    pub settings_url: String,
    pub storage_url: String,
    pub sync_status: SyncStatus,
    pub active_task_count: usize,
}

/// Drive information for the settings UI
#[derive(Debug, Clone, Serialize)]
pub struct DriveInfo {
    /// Drive ID
    pub id: String,
    /// Drive display name
    pub name: String,
    /// Instance URL
    pub instance_url: String,
    pub remote_path: String,
    /// Local sync path
    pub sync_path: String,
    /// Path to the ICO icon
    pub icon_path: Option<String>,
    /// Path to the raw (non-ICO) icon image
    pub raw_icon_path: Option<String>,
    /// Whether the drive is enabled
    pub enabled: bool,
    /// User ID
    pub user_id: String,
    /// Current drive status
    pub status: DriveInfoStatus,
    /// Capacity summary (None if not available)
    pub capacity: Option<CapacitySummary>,
}

/// Drive status for the settings UI
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveInfoStatus {
    /// Drive is active and synced
    Active,
    // Event push subscription is lost
    EventPushLost,
    /// Credentials have expired
    CredentialExpired,
}

/// Format bytes into a human-readable string (e.g., "1.5 GB")
pub fn format_bytes(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    let bytes_f = bytes as f64;

    if bytes_f >= TB {
        format!("{:.1} TB", bytes_f / TB)
    } else if bytes_f >= GB {
        format!("{:.1} GB", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.1} MB", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.1} KB", bytes_f / KB)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod format_bytes_tests {
    use super::format_bytes;

    #[test]
    fn small_counts_as_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
    }

    #[test]
    fn kb_mb_gb_tb() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024_i64.pow(3)), "1.0 GB");
        assert_eq!(format_bytes(1024_i64.pow(4)), "1.0 TB");
    }
}
