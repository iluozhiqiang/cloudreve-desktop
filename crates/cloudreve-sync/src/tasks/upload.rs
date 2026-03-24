use std::{path::PathBuf, str::FromStr, sync::Arc};

use crate::{
    drive::{placeholder::CrPlaceholder, utils::local_path_to_cr_uri},
    inventory::{ConflictState, FileMetadata, InventoryDb},
    platform_provider,
    tasks::queue::QueuedTask,
    uploader::{ProgressCallback, ProgressUpdate, UploadParams, Uploader, UploaderConfig},
};
use anyhow::{Context, Result};
use bytes::Bytes;
use chrono::DateTime;
use cloudreve_api::{
    ApiError, Client,
    api::ExplorerApi,
    error::ErrorCode,
    models::explorer::{
        CreateFileService, FileResponse, FileUpdateService, GetFileInfoService, file_type,
    },
};
use cloudreve_platforms_api::{VirtualFileMode, VirtualFileState};
use dashmap::DashMap;
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::types::TaskProgress;

fn is_api_error_code_in_chain(err: &anyhow::Error, want: i32) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<ApiError>()
            .is_some_and(|api_err| matches!(api_err, ApiError::ApiError { code, .. } if *code == want))
    })
}

/// Remote `updated_at` vs local mtime may differ by 1–2s (FS / server rounding).
const OBJECT_EXISTED_MTIME_TOLERANCE_SECS: i64 = 2;

#[derive(Debug, Error)]
#[error("remote file exists but size or modification time does not match local")]
struct ObjectExistedMetadataMismatch;

/// When the server reports 40004, we only auto-resolve if local and remote look like the same content.
fn local_matches_remote_object_existed(file: &FileResponse, local: &VirtualFileState) -> bool {
    if !local.exists {
        return false;
    }
    if local.is_directory != (file.file_type == file_type::FOLDER) {
        return false;
    }
    let remote_size = file.size;
    let Some(local_size) = local.file_size.map(|s| s as i64) else {
        warn!(target: "tasks::upload", "ObjectExisted compare: missing local file_size");
        return false;
    };
    if local_size != remote_size {
        info!(
            target: "tasks::upload",
            local_size,
            remote_size,
            "ObjectExisted compare: size mismatch"
        );
        return false;
    }

    let remote_ts = DateTime::parse_from_rfc3339(&file.updated_at)
        .ok()
        .map(|dt| dt.timestamp());
    let local_ts = local.last_modified_unix;

    match (remote_ts, local_ts) {
        (Some(r), Some(l)) => {
            let ok = (r - l).abs() <= OBJECT_EXISTED_MTIME_TOLERANCE_SECS;
            if !ok {
                info!(
                    target: "tasks::upload",
                    remote_mtime = r,
                    local_mtime = l,
                    "ObjectExisted compare: mtime outside tolerance"
                );
            }
            ok
        }
        _ => {
            info!(
                target: "tasks::upload",
                "ObjectExisted compare: missing mtime on one side; accepting size-only match"
            );
            true
        }
    }
}

/// Progress reporter that updates task progress in-memory via a DashMap reference.
/// Does NOT persist to inventory - only keeps in-memory for real-time queries.
pub struct InMemoryProgressReporter {
    task_id: String,
    progress_map: Arc<DashMap<String, TaskProgress>>,
}

impl InMemoryProgressReporter {
    pub fn new(task_id: String, progress_map: Arc<DashMap<String, TaskProgress>>) -> Self {
        Self {
            task_id,
            progress_map,
        }
    }
}

impl ProgressCallback for InMemoryProgressReporter {
    fn on_progress(&self, update: ProgressUpdate) {
        if let Some(mut entry) = self.progress_map.get_mut(&self.task_id) {
            entry.update_from_progress(&update);
        }
    }
}

pub struct UploadTask<'a> {
    inventory: Arc<InventoryDb>,
    cr_client: Arc<Client>,
    drive_id: &'a str,
    sync_path: PathBuf,
    remote_base: String,
    task: &'a QueuedTask,
    local_file: Option<CrPlaceholder>,
    inventory_meta: Option<FileMetadata>,
    cancel_token: CancellationToken,
    /// Reference to the in-memory progress map for real-time progress updates
    progress_map: Arc<DashMap<String, TaskProgress>>,
}

impl<'a> UploadTask<'a> {
    pub fn new(
        inventory: Arc<InventoryDb>,
        cr_client: Arc<Client>,
        drive_id: &'a str,
        task: &'a QueuedTask,
        sync_path: PathBuf,
        remote_base: String,
        progress_map: Arc<DashMap<String, TaskProgress>>,
    ) -> Self {
        Self {
            inventory,
            cr_client,
            drive_id,
            local_file: None,
            inventory_meta: None,
            task,
            sync_path,
            remote_base,
            cancel_token: CancellationToken::new(),
            progress_map,
        }
    }

    /// Set the cancellation token
    #[allow(dead_code)]
    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = token;
        self
    }

    // Upload a local file/folder to cloud
    pub async fn execute(&mut self) -> Result<()> {
        // Get local file info
        let placeholder_file = CrPlaceholder::new(
            &self.task.payload.local_path,
            self.sync_path.clone(),
            Uuid::from_str(self.drive_id)?,
        );
        if !placeholder_file.local_file_info.exists {
            info!(
                target: "tasks::upload",
                task_id = %self.task.task_id,
                local_path = %self.task.payload.local_path_display(),
                "Local file does not exist, skipping upload"
            );
            return Ok(());
        }

        // `VirtualFileState::in_sync` comes from the platform (e.g. Win CFAPI / macOS markers).
        // On macOS simple sync (`VirtualFileMode::None`), plain files have no marker and incorrectly
        // report `in_sync == true`, which would skip the real upload while still completing the task.
        let trust_platform_in_sync_skip = platform_provider()
            .map(|p| p.virtual_files().mode() != VirtualFileMode::None)
            .unwrap_or(false);

        if trust_platform_in_sync_skip
            && placeholder_file.local_file_info.is_in_sync()
            && !placeholder_file.local_file_info.is_directory()
        {
            info!(
                target: "tasks::upload",
                task_id = %self.task.task_id,
                local_path = %self.task.payload.local_path_display(),
                "Local file is in sync, skipping upload"
            );
            return Ok(());
        }

        let is_directory = placeholder_file.local_file_info.is_directory;
        let file_size = placeholder_file.local_file_info.file_size.unwrap_or(0);
        self.local_file = Some(placeholder_file);

        // Get inventory meta
        let path_str = self
            .task
            .payload
            .local_path
            .to_str()
            .context("failed to get local path as str")?;
        self.inventory_meta = self
            .inventory
            .query_by_path(path_str)
            .context("failed to get inventory meta")?;

        // clear file error state
        // Mark file as error state
        if let Err(e) = self
            .local_file
            .as_mut()
            .unwrap()
            .update_sync_error_state(false)
        {
            warn!(target: "tasks::upload", task_id = %self.task.task_id, local_path = %self.task.payload.local_path_display(), error = ?e, "Failed to clear sync error state");
        }

        // Handle empty files and directories separately
        let upload_res = match (
            is_directory,
            file_size == 0 && !self.task.payload.force_override,
            self.inventory_meta.is_none(),
        ) {
            (true, _, _) => self.create_empty_file_or_folder().await,
            (false, true, true) => self.create_empty_file_or_folder().await,
            (false, true, false) => self.clear_file_content().await,
            (false, false, _) => self.upload_file_with_uploader().await,
        };

        self.handle_error(upload_res).await
    }

    fn mark_conflict_pending_and_notify(&self) {
        let path_str = self.task.payload.local_path.to_str().unwrap_or_default();
        if let Err(mark_err) = self
            .inventory
            .mark_as_conflicted(path_str, Some(ConflictState::Pending))
        {
            warn!(
                target: "tasks::upload",
                task_id = %self.task.task_id,
                local_path = %self.task.payload.local_path_display(),
                error = ?mark_err,
                "Failed to mark file as conflicted"
            );
        }

        if let Ok(platform) = platform_provider() {
            platform.desktop_integration().send_conflict_notification(
                self.drive_id,
                &self.task.payload.local_path,
                self.inventory_meta
                    .as_ref()
                    .map(|meta| meta.id)
                    .unwrap_or(0),
            )
        }
    }

    async fn handle_error(&mut self, r: Result<()>) -> Result<()> {
        match r {
            Ok(()) => Ok(()),
            Err(e) => {
                // Remote already has this path — only treat as success if local size/mtime match remote.
                if is_api_error_code_in_chain(&e, ErrorCode::ObjectExisted as i32) {
                    info!(
                        target: "tasks::upload",
                        task_id = %self.task.task_id,
                        local_path = %self.task.payload.local_path_display(),
                        "Object already exists on server; checking metadata before aligning inventory"
                    );
                    match self.align_inventory_on_object_existed().await {
                        Ok(()) => return Ok(()),
                        Err(align_err) if align_err.is::<ObjectExistedMetadataMismatch>() => {
                            warn!(
                                target: "tasks::upload",
                                task_id = %self.task.task_id,
                                local_path = %self.task.payload.local_path_display(),
                                "ObjectExisted but local vs remote size/mtime differ — conflict"
                            );
                            self.mark_conflict_pending_and_notify();
                            if let Err(state_err) = self
                                .local_file
                                .as_mut()
                                .unwrap()
                                .update_sync_error_state(true)
                            {
                                warn!(target: "tasks::upload", task_id = %self.task.task_id, error = ?state_err, "Failed to update sync error state");
                            }
                            return Err(align_err.context(e));
                        }
                        Err(align_err) => {
                            warn!(
                                target: "tasks::upload",
                                task_id = %self.task.task_id,
                                error = %align_err,
                                "Failed to align inventory after ObjectExisted; surfacing original error"
                            );
                        }
                    }
                }

                // StaleVersion (40076): true version conflict — keep conflict flow.
                let is_stale_version = is_api_error_code_in_chain(&e, ErrorCode::StaleVersion as i32);

                if is_stale_version {
                    warn!(
                        target: "tasks::upload",
                        task_id = %self.task.task_id,
                        local_path = %self.task.payload.local_path_display(),
                        "Stale version / conflict with server"
                    );
                    self.mark_conflict_pending_and_notify();
                }

                if let Err(state_err) = self
                    .local_file
                    .as_mut()
                    .unwrap()
                    .update_sync_error_state(true)
                {
                    warn!(target: "tasks::upload", task_id = %self.task.task_id, local_path = %self.task.payload.local_path_display(), error = ?state_err, "Failed to update sync error state");
                }
                Err(e)
            }
        }
    }

    async fn clear_file_content(&mut self) -> Result<()> {
        info!(
            target: "tasks::upload",
            task_id = %self.task.task_id,
            local_path = %self.task.payload.local_path_display(),
            "Clearing file content with update request"
        );

        let uri = local_path_to_cr_uri(
            self.task.payload.local_path.clone(),
            self.sync_path.clone(),
            self.remote_base.clone(),
        )
        .context("failed to convert local path to cloudreve uri")?
        .to_string();
        let etag = self.inventory_meta.as_ref().unwrap().etag.clone();
        let res = self
            .cr_client
            .update_file(
                &FileUpdateService {
                    uri,
                    previous: Some(etag),
                },
                Bytes::new(),
            )
            .await;

        match res {
            Ok(file) => self.file_uploaded(&file),
            Err(e) => Err(e.into()),
        }
    }

    /// Upload a file using the new uploader module
    async fn upload_file_with_uploader(&mut self) -> Result<()> {
        let local_file = self.local_file.as_ref().unwrap();
        let file_size = local_file.local_file_info.file_size.unwrap_or(0);
        let is_new_file = self.inventory_meta.is_none();

        info!(
            target: "tasks::upload",
            task_id = %self.task.task_id,
            local_path = %self.task.payload.local_path_display(),
            file_size = file_size,
            "Starting file upload"
        );

        // Get remote URI
        let uri = local_path_to_cr_uri(
            self.task.payload.local_path.clone(),
            self.sync_path.clone(),
            self.remote_base.clone(),
        )
        .context("failed to convert local path to cloudreve uri")?
        .to_string();

        // If conflict state is set to Override, omit previous_version to force upload without version check
        let previous_version = if let Some(meta) = &self.inventory_meta {
            if matches!(meta.conflict_state, Some(ConflictState::Override)) {
                String::new() // Omit previous version when user chose to override
            } else {
                meta.etag.clone()
            }
        } else {
            String::new()
        };

        let params = UploadParams {
            local_path: self.task.payload.local_path.clone(),
            remote_uri: uri,
            file_size,
            mime_type: None, // Could be detected from file extension
            last_modified: local_file
                .local_file_info
                .last_modified_unix
                .map(|timestamp| timestamp.saturating_mul(1000)),
            overwrite: !is_new_file || self.task.payload.force_override,
            previous_version,
            task_id: self.task.task_id.clone(),
            drive_id: self.drive_id.to_string(),
        };

        // Create uploader configuration
        let config = UploaderConfig::default();

        // Create uploader
        let uploader = Uploader::new(self.cr_client.clone(), self.inventory.clone(), config)
            .with_cancel_token(self.cancel_token.clone());

        // Create in-memory progress reporter (does not persist to inventory)
        let progress = InMemoryProgressReporter::new(
            self.task.task_id.clone(),
            Arc::clone(&self.progress_map),
        );

        // Execute upload
        uploader
            .upload(params, progress)
            .await
            .context("failed to upload file")?;

        // Update local file placeholder status after successful upload
        self.finalize_upload().await?;

        Ok(())
    }

    async fn fetch_remote_file_info(&self) -> Result<FileResponse> {
        let uri = local_path_to_cr_uri(
            self.task.payload.local_path.clone(),
            self.sync_path.clone(),
            self.remote_base.clone(),
        )
        .context("failed to convert local path to cloudreve uri")?
        .to_string();

        self.cr_client
            .get_file_info(&GetFileInfoService {
                uri: Some(uri),
                id: None,
                extended: None,
                folder_summary: None,
            })
            .await
            .context("failed to get file info from server")
    }

    /// After a successful chunked upload, pull remote row and commit (no local compare).
    async fn pull_remote_into_inventory(&mut self) -> Result<()> {
        let file_info = self.fetch_remote_file_info().await?;
        self.file_uploaded(&file_info)
            .context("failed to commit remote file metadata")?;
        Ok(())
    }

    /// 40004: remote path exists — only auto-resolve if size and mtime match local (within tolerance).
    async fn align_inventory_on_object_existed(&mut self) -> Result<()> {
        let file_info = self.fetch_remote_file_info().await?;
        let local = &self
            .local_file
            .as_ref()
            .context("local_file missing for ObjectExisted align")?
            .local_file_info;

        if !local_matches_remote_object_existed(&file_info, local) {
            return Err(ObjectExistedMetadataMismatch.into());
        }

        self.file_uploaded(&file_info)
            .context("failed to commit remote file metadata")?;
        Ok(())
    }

    /// Finalize upload by updating local file placeholder
    async fn finalize_upload(&mut self) -> Result<()> {
        self.pull_remote_into_inventory().await
    }

    async fn create_empty_file_or_folder(&mut self) -> Result<()> {
        info!(
            target: "tasks::upload",
            task_id = %self.task.task_id,
            local_path = %self.task.payload.local_path_display(),
            "Creating empty file/folder"
        );
        let local_file = &self.local_file.as_ref().unwrap().local_file_info;
        let uri = local_path_to_cr_uri(
            self.task.payload.local_path.clone(),
            self.sync_path.clone(),
            self.remote_base.clone(),
        )
        .context("failed to convert local path to cloudreve uri")?
        .to_string();

        debug!(target: "tasks::upload", task_id = %self.task.task_id, local_path = %self.task.payload.local_path_display(), uri = %uri, "Send test toast");

        // Create file in remote
        let res = self
            .cr_client
            .create_file(&CreateFileService {
                uri,
                file_type: if local_file.is_directory {
                    "folder".to_string()
                } else {
                    "file".to_string()
                },
                err_on_conflict: Some(!local_file.is_directory),
                metadata: None,
            })
            .await;
        match res {
            Ok(folder) => self.file_uploaded(&folder),
            Err(e) => {
                if matches!(
                    &e,
                    ApiError::ApiError { code, .. } if *code == ErrorCode::ObjectExisted as i32
                ) {
                    info!(
                        target: "tasks::upload",
                        task_id = %self.task.task_id,
                        local_path = %self.task.payload.local_path_display(),
                        "Create returned ObjectExisted; aligning inventory"
                    );
                    return self.align_inventory_on_object_existed().await;
                }
                Err(e.into())
            }
        }
    }

    fn file_uploaded(&mut self, file: &FileResponse) -> Result<()> {
        info!(
            target: "tasks::upload",
            task_id = %self.task.task_id,
            local_path = %self.task.payload.local_path_display(),
            "File uploaded"
        );

        self.local_file = Some(
            self.local_file
                .take()
                .unwrap()
                .with_mark_no_children(file.file_type == file_type::FOLDER)
                .with_remote_file(file),
        );

        self.local_file
            .as_mut()
            .unwrap()
            .commit(self.inventory.clone())
            .context("failed to commit placeholder")?;

        self.local_file
            .as_mut()
            .unwrap()
            .update_sync_error_state(false)
            .context("failed to clear sync error state")?;
        Ok(())
    }
}
