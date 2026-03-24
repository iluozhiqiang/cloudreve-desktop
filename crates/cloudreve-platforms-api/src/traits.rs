use std::{
    fmt,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;

use crate::types::{
    FileProviderItemState, LocalAvailability, PlaceholderEntry, PlatformCapabilities,
    VirtualFileMetadata, VirtualFileMode, VirtualFileState, VirtualPlaceholderSpec,
};

pub trait MountSession: Send + Sync {
    fn disconnect(&self) -> Result<()>;
}

pub trait FetchDataWriter: Send + Sync {
    fn write_at(&self, data: &[u8], offset: u64) -> Result<()>;
    fn report_progress(&self, total_bytes: u64, transferred_bytes: u64) -> Result<()>;
}

#[derive(Clone)]
pub struct FetchDataRequest {
    pub path: PathBuf,
    pub range: Range<u64>,
    pub writer: Arc<dyn FetchDataWriter>,
}

impl fmt::Debug for FetchDataRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FetchDataRequest")
            .field("path", &self.path)
            .field("range", &self.range)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct MountRegistrationCustomState {
    pub id: u32,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct MountRegistrationContext {
    pub mount_id: String,
    pub display_name: String,
    pub sync_path: PathBuf,
    pub icon_path: Option<String>,
    pub recycle_bin_url: Option<String>,
    pub custom_states: Vec<MountRegistrationCustomState>,
}

pub trait MountedDriveCallback: Send + Sync {
    fn fetch_data(&self, request: FetchDataRequest) -> Result<()>;
    fn fetch_placeholders(&self, path: PathBuf) -> Result<Vec<PlaceholderEntry>>;
    /// Query system-visible per-item state for Finder/File Provider overlays.
    fn get_item_state(&self, path: PathBuf) -> Result<FileProviderItemState>;
    fn rename(&self, source: PathBuf, target: PathBuf) -> Result<()>;
    fn renamed(&self, source: PathBuf, destination: PathBuf);
}

pub trait PlatformMountProvider: Send + Sync {
    fn is_supported(&self) -> Result<bool>;
    fn ensure_mount_id(
        &self,
        instance_url: &str,
        user_id: &str,
        sync_path: &Path,
    ) -> Result<String>;
    fn connect_mount(
        &self,
        context: &MountRegistrationContext,
        handler: Arc<dyn MountedDriveCallback>,
    ) -> Result<Box<dyn MountSession>>;
    fn unregister_mount(&self, mount_id: &str) -> Result<()>;
}

pub trait VirtualFileProvider: Send + Sync {
    fn mode(&self) -> VirtualFileMode;
}

pub trait VirtualFileOps: Send + Sync {
    fn query_local_state(&self, path: &Path) -> Result<VirtualFileState>;
    fn upsert_placeholder(&self, spec: &VirtualPlaceholderSpec) -> Result<()>;
    fn remove_placeholder(&self, path: &Path) -> Result<()>;
    fn mark_in_sync(&self, path: &Path, in_sync: bool) -> Result<()>;
    fn hydrate_file(&self, path: &Path, range: Range<u64>) -> Result<()>;
    fn dehydrate_file(&self, path: &Path, range: Range<u64>) -> Result<()>;
    fn set_local_availability(&self, path: &Path, availability: LocalAvailability) -> Result<()>;
    fn set_sync_error(&self, path: &Path, has_error: bool) -> Result<()>;
    fn metadata_from_unix(
        &self,
        size: u64,
        created_unix: i64,
        modified_unix: i64,
    ) -> Result<VirtualFileMetadata> {
        Ok(VirtualFileMetadata {
            size,
            created_unix,
            modified_unix,
        })
    }
}

pub trait DesktopIntegration: Send + Sync {
    fn send_general_text_notification(&self, title: &str, message: &str);
    fn send_token_expiry_notification(&self, drive_id: &str, title: &str, message: &str);
    fn send_conflict_notification(&self, drive_id: &str, path: &Path, inventory_id: i64);
    fn open_in_file_manager(&self, path: &Path) -> Result<()>;
}

pub trait AutoStartProvider: Send + Sync {
    fn is_enabled(&self) -> Result<bool>;
    fn set_enabled(&self, enabled: bool) -> Result<bool>;
}

pub trait PlatformProvider: Send + Sync {
    fn capabilities(&self) -> PlatformCapabilities;
    fn mounts(&self) -> &dyn PlatformMountProvider;
    fn virtual_files(&self) -> &dyn VirtualFileProvider;
    fn virtual_file_ops(&self) -> &dyn VirtualFileOps;
    fn desktop_integration(&self) -> &dyn DesktopIntegration;
    fn auto_start(&self) -> &dyn AutoStartProvider;
}
