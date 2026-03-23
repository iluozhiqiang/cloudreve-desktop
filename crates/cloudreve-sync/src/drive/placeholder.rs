use crate::{
    inventory::{FileMetadata, InventoryDb, MetadataEntry},
    platform_provider,
};
use anyhow::{Context, Result};
use chrono::DateTime;
use cloudreve_api::models::explorer::{FileResponse, file_type};
use cloudreve_platforms_api::{
    VirtualFileMetadata, VirtualFileState, VirtualPlaceholderSpec,
};
use std::{
    ffi::OsString,
    path::PathBuf,
    sync::Arc,
};
use uuid::Uuid;

pub struct CrPlaceholder {
    pub local_file_info: VirtualFileState,

    local_path: PathBuf,
    sync_root: PathBuf,
    drive_id: Uuid,
    file_meta: Option<FileMetadata>,
    options: u32,
}

enum CrPlaceholderOptions {
    InvalidateAllRange = 1 << 0,
    MarkNoChildren = 1 << 1,
}

impl CrPlaceholder {
    pub fn new(local_path: impl Into<PathBuf>, sync_root: PathBuf, drive_id: Uuid) -> Self {
        let local_path = local_path.into();
        Self {
            local_path: local_path.clone(),
            sync_root,
            drive_id,
            file_meta: None,
            options: 0,
            local_file_info: platform_provider()
                .and_then(|platform| platform.virtual_file_ops().query_local_state(&local_path))
                .unwrap_or_else(|_| VirtualFileState::missing()),
        }
    }

    pub fn with_invalidate_all_range(mut self, enable: bool) -> Self {
        if enable {
            self.options |= CrPlaceholderOptions::InvalidateAllRange as u32;
        } else {
            self.options &= !(CrPlaceholderOptions::InvalidateAllRange as u32);
        }
        self
    }

    pub fn with_mark_no_children(mut self, enable: bool) -> Self {
        if enable {
            self.options |= CrPlaceholderOptions::MarkNoChildren as u32;
        } else {
            self.options &= !(CrPlaceholderOptions::MarkNoChildren as u32);
        }
        self
    }

    pub fn with_file_meta(mut self, file_meta: FileMetadata) -> Self {
        self.file_meta = Some(file_meta);
        self
    }

    pub fn delete_placeholder(&self, inventory: Arc<InventoryDb>) -> Result<()> {
        if let Ok(platform) = platform_provider() {
            platform
                .virtual_file_ops()
                .remove_placeholder(&self.local_path)
                .context("failed to remove local placeholder")?;
        }

        // Remove from inventory
        let path_str = self
            .local_path
            .to_str()
            .context("failed to convert path to string")?;
        inventory
            .batch_delete_by_path(vec![path_str])
            .context("failed to delete from inventory")?;

        Ok(())
    }

    // Commit changes to file system and inventory
    pub fn commit(&mut self, inventory: Arc<InventoryDb>) -> Result<()> {
        if self.file_meta.is_none() {
            return Err(anyhow::anyhow!("File metadata is not set"));
        }

        let file_meta = self.file_meta.as_ref().unwrap();

        let identity_blob = OsString::from(file_meta.etag.clone()).into_encoded_bytes();
        let spec = VirtualPlaceholderSpec {
            target_path: self.local_path.clone(),
            sync_root: self.sync_root.clone(),
            is_directory: file_meta.is_folder,
            metadata: VirtualFileMetadata {
                size: file_meta.size as u64,
                created_unix: file_meta.created_at,
                modified_unix: file_meta.updated_at,
            },
            identity_blob,
            mark_in_sync: true,
            overwrite: true,
            dehydrate_on_update: self.options & CrPlaceholderOptions::InvalidateAllRange as u32 != 0,
            has_no_children: self.options & CrPlaceholderOptions::MarkNoChildren as u32 != 0,
        };

        platform_provider()?
            .virtual_file_ops()
            .upsert_placeholder(&spec)
            .context("failed to upsert placeholder")?;

        // Upser inventory
        inventory
            .upsert(&MetadataEntry::from(file_meta))
            .context("failed to upsert inventory")?;

        Ok(())
    }

    pub fn with_remote_file(mut self, file_info: &FileResponse) -> Self {
        // Parse RFC3339 time strings from Golang
        let created_at = DateTime::parse_from_rfc3339(&file_info.created_at)
            .ok()
            .map(|dt| dt.timestamp())
            .unwrap_or_default();

        let updated_at = DateTime::parse_from_rfc3339(&file_info.updated_at)
            .ok()
            .map(|dt| dt.timestamp())
            .unwrap_or_default();

        let mut metadata = file_info.metadata.clone().unwrap_or_default();
        metadata.insert(
            crate::drive::utils::INVENTORY_REMOTE_URI_KEY.to_string(),
            file_info.path.clone(),
        );

        self.file_meta = Some(FileMetadata {
            drive_id: self.drive_id,
            local_path: self.local_path.to_string_lossy().to_string(),
            is_folder: file_info.file_type == file_type::FOLDER,
            created_at,
            updated_at,
            size: file_info.size,
            etag: file_info.primary_entity.clone().unwrap_or_default(),
            id: 0,
            metadata,
            props: None,
            permissions: file_info.permission.clone().unwrap_or_default(),
            shared: file_info.shared.unwrap_or(false),
            conflict_state: None,
        });
        self
    }

    /// Updates the platform-reported sync error state for a file or folder.
    ///
    /// The concrete effect depends on the active platform virtual file implementation.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the file or folder
    /// * `set_error` - If true, sets the error state (shows error overlay);
    ///                 if false, clears the error state
    ///
    /// # Example
    ///
    /// ```ignore
    /// // On a virtual placeholder handle:
    /// placeholder.update_sync_error_state(true)?;
    /// placeholder.update_sync_error_state(false)?;
    /// ```
    pub fn update_sync_error_state(&self, set_error: bool) -> Result<()> {
        if !self.local_file_info.is_virtual_placeholder() {
            // Skip non-placeholder file
            return Ok(());
        }
        platform_provider()?
            .virtual_file_ops()
            .set_sync_error(&self.local_path, set_error)
            .context("failed to update sync error state")?;

        tracing::debug!(
            target: "drive::placeholder",
            path = %self.local_path.display(),
            set_error,
            "Updated sync error state"
        );

        Ok(())
    }
}
