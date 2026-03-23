use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use cloudreve_api::{Boolset, models::explorer::file_permission};
use cloudreve_platforms_windows::shell_host::{
    CapacitySummary, ConflictResolutionAction, ConflictResolutionRequest, DriveStatusUI,
    ShellExtensionHost, ShellItemState, SyncStatus,
};
use cloudreve_sync::{
    DriveManager,
    drive::{
        commands::{ConflictAction, ManagerCommand},
        sync::SyncMode,
    },
    inventory::ConflictState,
};
use tokio::sync::oneshot;

pub struct WindowsShellHost {
    drive_manager: Arc<DriveManager>,
}

impl WindowsShellHost {
    pub fn new(drive_manager: Arc<DriveManager>) -> Self {
        Self { drive_manager }
    }
}

impl ShellExtensionHost for WindowsShellHost {
    fn get_shell_item_state(&self, path: &Path) -> Result<Option<ShellItemState>> {
        let Some(path_str) = path.to_str() else {
            return Ok(None);
        };

        let Some(metadata) = self
            .drive_manager
            .get_inventory()
            .query_by_path(path_str)
            .context("failed to query inventory by path")?
        else {
            return Ok(None);
        };

        let readable = if metadata.permissions.is_empty() {
            true
        } else {
            Boolset::from_base64(&metadata.permissions)
                .map(|permission| permission.enabled(file_permission::READ as usize))
                .unwrap_or(true)
        };

        Ok(Some(ShellItemState {
            shared: metadata.shared,
            readable,
            has_pending_conflict: matches!(metadata.conflict_state, Some(ConflictState::Pending)),
        }))
    }

    fn view_online(&self, path: PathBuf) -> Result<()> {
        self.drive_manager
            .get_command_sender()
            .send(ManagerCommand::ViewOnline { path })
            .map_err(|e| anyhow::anyhow!("failed to send ViewOnline command: {}", e))
    }

    fn sync_now(&self, paths: Vec<PathBuf>) -> Result<()> {
        self.drive_manager
            .get_command_sender()
            .send(ManagerCommand::SyncNow {
                paths,
                mode: SyncMode::FullHierarchy,
            })
            .map_err(|e| anyhow::anyhow!("failed to send SyncNow command: {}", e))
    }

    fn show_conflict_toast(&self, path: PathBuf) -> Result<()> {
        self.drive_manager
            .get_command_sender()
            .send(ManagerCommand::ShowConflictToast { path })
            .map_err(|e| anyhow::anyhow!("failed to send ShowConflictToast command: {}", e))
    }

    fn resolve_conflict(&self, request: ConflictResolutionRequest) -> Result<()> {
        let action = match request.action {
            ConflictResolutionAction::KeepRemote => ConflictAction::KeepRemote,
            ConflictResolutionAction::OverwriteRemote => ConflictAction::OverwriteRemote,
            ConflictResolutionAction::SaveAsNew => ConflictAction::SaveAsNew,
        };

        self.drive_manager
            .get_command_sender()
            .send(ManagerCommand::ResolveConflict {
                drive_id: request.drive_id,
                file_id: request.file_id,
                path: request.path,
                action,
            })
            .map_err(|e| anyhow::anyhow!("failed to send ResolveConflict command: {}", e))
    }

    fn generate_thumbnail(&self, path: PathBuf) -> Result<Vec<u8>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.drive_manager
            .get_command_sender()
            .send(ManagerCommand::GenerateThumbnail {
                path,
                response: response_tx,
            })
            .map_err(|e| anyhow::anyhow!("failed to send GenerateThumbnail command: {}", e))?;

        response_rx
            .blocking_recv()
            .context("failed to receive thumbnail response")?
            .map(|bytes| bytes.to_vec())
    }

    fn get_drive_status_ui(&self, mount_id: &str) -> Result<Option<DriveStatusUI>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.drive_manager
            .get_command_sender()
            .send(ManagerCommand::GetDriveStatusUI {
                mount_id: mount_id.to_string(),
                response: response_tx,
            })
            .map_err(|e| anyhow::anyhow!("failed to send GetDriveStatusUI command: {}", e))?;

        response_rx
            .blocking_recv()
            .context("failed to receive drive status ui response")?
            .map(|status| status.map(map_drive_status_ui))
    }

    fn open_sync_status_window(&self) -> Result<()> {
        self.drive_manager
            .get_command_sender()
            .send(ManagerCommand::OpenSyncStatusWindow)
            .map_err(|e| anyhow::anyhow!("failed to send OpenSyncStatusWindow command: {}", e))
    }

    fn open_settings_window(&self) -> Result<()> {
        self.drive_manager
            .get_command_sender()
            .send(ManagerCommand::OpenSettingsWindow)
            .map_err(|e| anyhow::anyhow!("failed to send OpenSettingsWindow command: {}", e))
    }

    fn open_profile_url(&self, mount_id: &str) -> Result<()> {
        self.drive_manager
            .get_command_sender()
            .send(ManagerCommand::OpenProfileUrl {
                mount_id: mount_id.to_string(),
            })
            .map_err(|e| anyhow::anyhow!("failed to send OpenProfileUrl command: {}", e))
    }

    fn open_storage_details_url(&self, mount_id: &str) -> Result<()> {
        self.drive_manager
            .get_command_sender()
            .send(ManagerCommand::OpenStorageDetailsUrl {
                mount_id: mount_id.to_string(),
            })
            .map_err(|e| anyhow::anyhow!("failed to send OpenStorageDetailsUrl command: {}", e))
    }

    fn register_status_ui_changed(
        &self,
        callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<()> {
        self.drive_manager.register_on_status_ui_changed(move || {
            callback();
        })
    }
}

fn map_drive_status_ui(status: cloudreve_sync::DriveStatusUI) -> DriveStatusUI {
    DriveStatusUI {
        name: status.name,
        raw_icon_path: status.raw_icon_path,
        capacity: status.capacity.map(map_capacity_summary),
        profile_url: status.profile_url,
        settings_url: status.settings_url,
        storage_url: status.storage_url,
        sync_status: map_sync_status(status.sync_status),
        active_task_count: status.active_task_count,
    }
}

fn map_capacity_summary(summary: cloudreve_sync::CapacitySummary) -> CapacitySummary {
    CapacitySummary {
        total: summary.total,
        used: summary.used,
        label: summary.label,
    }
}

fn map_sync_status(status: cloudreve_sync::SyncStatus) -> SyncStatus {
    match status {
        cloudreve_sync::SyncStatus::InSync => SyncStatus::InSync,
        cloudreve_sync::SyncStatus::Syncing => SyncStatus::Syncing,
        cloudreve_sync::SyncStatus::Paused => SyncStatus::Paused,
        cloudreve_sync::SyncStatus::Error => SyncStatus::Error,
    }
}
