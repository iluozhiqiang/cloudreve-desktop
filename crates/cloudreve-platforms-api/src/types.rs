use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Windows,
    Macos,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VirtualFileMode {
    None,
    Placeholder,
    FileProvider,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesktopIntegrationCapabilities {
    pub notifications: bool,
    pub status_ui: bool,
    pub file_manager_reveal: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlatformCapabilities {
    pub platform: PlatformKind,
    pub virtual_file_mode: VirtualFileMode,
    pub sync_root_registration: bool,
    pub auto_start: bool,
    pub desktop_integration: DesktopIntegrationCapabilities,
}

#[derive(Debug, Clone)]
pub struct PlaceholderEntry {
    pub relative_path: PathBuf,
    pub is_directory: bool,
    pub size: u64,
    pub created_unix: i64,
    pub modified_unix: i64,
    pub blob: Vec<u8>,
    pub mark_in_sync: bool,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalAvailability {
    Unspecified,
    OnlineOnly,
    AlwaysLocal,
}

#[derive(Debug, Clone)]
pub struct VirtualFileState {
    pub exists: bool,
    pub is_directory: bool,
    pub file_size: Option<u64>,
    pub last_modified_unix: Option<i64>,
    pub is_virtual_placeholder: bool,
    pub in_sync: bool,
    pub partially_on_disk: bool,
    pub local_availability: LocalAvailability,
    pub children_present: bool,
}

impl VirtualFileState {
    pub fn missing() -> Self {
        Self {
            exists: false,
            is_directory: false,
            file_size: None,
            last_modified_unix: None,
            is_virtual_placeholder: false,
            in_sync: false,
            partially_on_disk: false,
            local_availability: LocalAvailability::Unspecified,
            children_present: false,
        }
    }

    pub fn is_directory(&self) -> bool {
        self.is_directory
    }

    pub fn is_virtual_placeholder(&self) -> bool {
        self.is_virtual_placeholder
    }

    pub fn is_in_sync(&self) -> bool {
        self.in_sync
    }

    pub fn is_partially_on_disk(&self) -> bool {
        self.partially_on_disk
    }

    pub fn has_materialized_children(&self) -> bool {
        self.children_present
    }
}

#[derive(Debug, Clone)]
pub struct VirtualFileMetadata {
    pub size: u64,
    pub created_unix: i64,
    pub modified_unix: i64,
}

#[derive(Debug, Clone)]
pub struct VirtualPlaceholderSpec {
    pub target_path: PathBuf,
    pub sync_root: PathBuf,
    pub is_directory: bool,
    pub metadata: VirtualFileMetadata,
    pub identity_blob: Vec<u8>,
    pub mark_in_sync: bool,
    pub overwrite: bool,
    pub dehydrate_on_update: bool,
    pub has_no_children: bool,
}

/// System-level per-item state for macOS Finder overlays (File Provider / FPE)
/// as well as other future platform integrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FileProviderItemState {
    #[serde(rename = "CloudOnly")]
    CloudOnly,
    #[serde(rename = "Syncing")]
    Syncing,
    #[serde(rename = "Synced")]
    Synced,
    #[serde(rename = "Error")]
    Error,
}
