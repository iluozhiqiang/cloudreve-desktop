use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct CapacitySummary {
    pub total: i64,
    pub used: i64,
    pub label: String,
}

#[derive(Debug, Clone)]
pub enum SyncStatus {
    InSync,
    Syncing,
    Paused,
    Error,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum ConflictResolutionAction {
    KeepRemote,
    OverwriteRemote,
    SaveAsNew,
}

#[derive(Debug, Clone)]
pub struct ConflictResolutionRequest {
    pub drive_id: String,
    pub file_id: i64,
    pub path: String,
    pub action: ConflictResolutionAction,
}

#[derive(Debug, Clone)]
pub struct ShellItemState {
    pub shared: bool,
    pub readable: bool,
    pub has_pending_conflict: bool,
}

pub trait ShellExtensionHost: Send + Sync {
    fn get_shell_item_state(&self, path: &Path) -> Result<Option<ShellItemState>>;
    fn view_online(&self, path: PathBuf) -> Result<()>;
    fn sync_now(&self, paths: Vec<PathBuf>) -> Result<()>;
    fn show_conflict_toast(&self, path: PathBuf) -> Result<()>;
    fn resolve_conflict(&self, request: ConflictResolutionRequest) -> Result<()>;
    fn generate_thumbnail(&self, path: PathBuf) -> Result<Vec<u8>>;
    fn get_drive_status_ui(&self, mount_id: &str) -> Result<Option<DriveStatusUI>>;
    fn open_sync_status_window(&self) -> Result<()>;
    fn open_settings_window(&self) -> Result<()>;
    fn open_profile_url(&self, mount_id: &str) -> Result<()>;
    fn open_storage_details_url(&self, mount_id: &str) -> Result<()>;
    fn register_status_ui_changed(
        &self,
        callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<()>;
}
