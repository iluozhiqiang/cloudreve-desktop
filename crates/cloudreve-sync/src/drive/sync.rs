use crate::{
    drive::{
        mounts::Mount,
        placeholder::CrPlaceholder,
        utils::{
            local_bytes_match_inventory, local_path_to_cr_uri, remote_path_to_local_relative_path,
        },
    },
    inventory::{ConflictState, FileMetadata, MetadataEntry},
    is_real_file_sync_mode,
    tasks::TaskPayload,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cloudreve_platforms_api::{LocalAvailability, PlaceholderEntry, VirtualFileState};
use cloudreve_api::{
    ApiError,
    api::explorer::ExplorerApiExt,
    error::ErrorCode,
    models::{
        explorer::{FileResponse, file_type, metadata},
        uri::CrUri,
    },
};
use notify_debouncer_full::notify::event::{
    AccessKind, CreateKind, EventKind, ModifyKind, RemoveKind, RenameMode,
};
use notify_debouncer_full::{DebouncedEvent, notify::Event};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fmt, fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::task;
use uuid::Uuid;

pub fn cloud_file_to_placeholder_entry(
    file: &FileResponse,
    remote_path: &CrUri,
) -> Result<PlaceholderEntry> {
    let file_uri = CrUri::new(&file.path)?;
    let relative_path = remote_path_to_local_relative_path(&file_uri, &remote_path)?;
    let created_unix = file.created_at.parse::<DateTime<Utc>>()?.timestamp();
    let modified_unix = file.updated_at.parse::<DateTime<Utc>>()?.timestamp();
    let primary_entity = OsString::from(file.primary_entity.as_ref().unwrap_or(&String::new()));

    Ok(PlaceholderEntry {
        relative_path,
        is_directory: file.file_type == file_type::FOLDER,
        size: file.size as u64,
        created_unix,
        modified_unix,
        blob: primary_entity.into_encoded_bytes(),
        mark_in_sync: true,
        overwrite: true,
    })
}

/// Like [`cloud_file_to_placeholder_entry`], but merges `size` with inventory when the list API
/// returns stale `0` (e.g. server-side upload still finalizing). Finder reads this for `documentSize`.
pub fn cloud_file_to_placeholder_entry_merged(
    file: &FileResponse,
    remote_path: &CrUri,
    inventory: Option<&FileMetadata>,
) -> Result<PlaceholderEntry> {
    let mut entry = cloud_file_to_placeholder_entry(file, remote_path)?;
    if !entry.is_directory {
        let list_sz = file.size;
        let inv_sz = inventory.map(|m| m.size).unwrap_or(0);
        let merged = list_sz.max(inv_sz);
        entry.size = merged.max(0) as u64;
    }
    Ok(entry)
}

pub fn cloud_file_to_metadata_entry(
    file: &FileResponse,
    drive_id: &Uuid,
    local_path: &PathBuf,
) -> Result<MetadataEntry> {
    let mut local_path = local_path.clone();
    local_path.push(file.name.clone());
    let local_path_str = local_path.to_str();
    if local_path_str.is_none() {
        tracing::error!(
            target: "drive::mounts",
            local_path = %local_path.display(),
            error = "Failed to convert local path to string"
        );
        return Err(anyhow::anyhow!("Failed to convert local path to string"));
    }

    // Parse RFC time string to unix timestamp
    let created_at = file.created_at.parse::<DateTime<Utc>>()?.timestamp();
    let last_modified = file.updated_at.parse::<DateTime<Utc>>()?.timestamp();

    let mut metadata = file.metadata.clone().unwrap_or_default();
    metadata.insert(
        crate::drive::utils::INVENTORY_REMOTE_URI_KEY.to_string(),
        file.path.clone(),
    );

    Ok(MetadataEntry::new(
        drive_id.clone(),
        local_path_str.unwrap(),
        file.file_type == file_type::FOLDER,
    )
    .with_created_at(created_at)
    .with_updated_at(last_modified)
    .with_permissions(file.permission.as_ref().unwrap_or(&String::new()).clone())
    .with_shared(file.shared.unwrap_or(false))
    .with_size(file.size)
    .with_etag(
        file.primary_entity
            .as_ref()
            .unwrap_or(&String::new())
            .clone(),
    )
    .with_metadata(metadata))
}

pub fn is_symbolic_link(file: &FileResponse) -> bool {
    return file.metadata.is_some()
        && file
            .metadata
            .as_ref()
            .unwrap()
            .get(metadata::SHARE_REDIRECT)
            .is_some();
}

pub type GroupedFsEvents = HashMap<EventKind, Vec<Event>>;

const REMOTE_PAGE_SIZE: i32 = 1000;

/// Groups filesystem events by their first-level EventKind.
///
/// This function groups events into a HashMap where the key is the first-level EventKind
/// (normalized to use ::Any for nested variants) and the value is a vector of events.
///
/// # Arguments
/// * `events` - A vector of DebouncedEvent to be grouped
///
/// # Returns
/// A HashMap mapping EventKind to Vec<DebouncedEvent>
pub fn group_fs_events(events: Vec<DebouncedEvent>) -> GroupedFsEvents {
    let mut grouped: GroupedFsEvents = HashMap::new();

    for event in events {
        let normalized_kind = normalize_event_kind(&event.kind);
        grouped
            .entry(normalized_kind)
            .or_insert_with(Vec::new)
            .push(event.event);
    }

    grouped
}

/// Normalizes an EventKind to its first-level representation.
///
/// This helper function converts all nested EventKind variants to use their ::Any variant,
/// effectively grouping by the first level only. This can be extended to support deeper
/// level matching by adding parameters for match depth or specific variant matching.
///
/// # Arguments
/// * `kind` - The EventKind to normalize
///
/// # Returns
/// A normalized EventKind representing the first level only
fn normalize_event_kind(kind: &EventKind) -> EventKind {
    match kind {
        EventKind::Any => EventKind::Any,
        EventKind::Access(_) => EventKind::Access(AccessKind::Any),
        EventKind::Create(_) => EventKind::Create(CreateKind::Any),
        EventKind::Modify(modify_kind) => match modify_kind {
            ModifyKind::Name(rename_mode) => match rename_mode {
                RenameMode::Both => EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                _ => EventKind::Modify(ModifyKind::Any),
            },
            _ => EventKind::Modify(ModifyKind::Any),
        },
        EventKind::Remove(_) => EventKind::Remove(RemoveKind::Any),
        EventKind::Other => EventKind::Other,
    }
}

/// Determines how deep a sync operation should traverse for a given path list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Sync only the provided path entries.
    PathOnly,
    /// Sync paths with remote-delete intent (prefer deleting local when remote is missing).
    RemoteDelete,
    /// Sync the provided path entries and their first-level children.
    PathAndFirstLayer,
    /// Sync the provided path entries and every descendant.
    FullHierarchy,
}

const CONFLICT_PREFIX: &str = "__conflict__";

/// Real-file sync: skip creating a local file while the listing still has `size == 0` (upload may be in progress).
fn skip_none_mode_materialize_zero_byte_remote_file(remote: &FileResponse) -> bool {
    is_real_file_sync_mode()
        && remote.file_type != file_type::FOLDER
        && remote.size == 0
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum SyncAction {
    CreatePlaceholderAndInventory {
        path: PathBuf,
        remote: FileResponse,
    },
    // Update inventory and placehodler metadata, conver to placehodler if it's not one
    UpdateInventoryFromRemote {
        path: PathBuf,
        remote: FileResponse,
        invalidate_all: bool,
    },
    QueueUpload {
        path: PathBuf,
        reason: UploadReason,
    },
    QueueDownload {
        path: PathBuf,
        remote: FileResponse,
    },
    DeleteLocalAndInventory {
        path: PathBuf,
        skip_if_not_empty: bool,
    },
    CreateRemoteFolderIfExist {
        path: PathBuf,
    },
    RenameLocalWithConflict {
        original: PathBuf,
        renamed: PathBuf,
    },
}

#[derive(Debug, Clone, Copy)]
enum UploadReason {
    RemoteMismatch,
    RemoteMissing,
}

#[derive(Debug, Clone, Copy)]
enum WalkReason {
    ModePropagation,
    DiffTriggered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkTiming {
    Immediate,
    Deferred,
}

#[derive(Debug, Clone)]
struct WalkRequest {
    path: PathBuf,
    mode: SyncMode,
    reason: WalkReason,
    timing: WalkTiming,
}

#[derive(Default)]
struct SyncPlan {
    actions: Vec<SyncAction>,
    walk_requests: Vec<WalkRequest>,
}

// Debug print for SyncPlan
impl fmt::Debug for SyncPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "SyncPlan ({} actions, {} walks):",
            self.actions.len(),
            self.walk_requests.len()
        )?;

        for (i, action) in self.actions.iter().enumerate() {
            writeln!(f, "  [{}] {:?}", i, action)?;
        }

        for (i, walk) in self.walk_requests.iter().enumerate() {
            writeln!(f, "  [W{}] {:?}", i, walk)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
struct SyncErrorEntry {
    path: PathBuf,
    error: anyhow::Error,
}

#[derive(Debug)]
struct SyncAggregateError {
    context: String,
    entries: Vec<SyncErrorEntry>,
}

impl SyncAggregateError {
    fn new(context: impl Into<String>) -> Self {
        Self {
            context: context.into(),
            entries: Vec::new(),
        }
    }

    fn push<E>(&mut self, path: PathBuf, error: E)
    where
        E: Into<anyhow::Error>,
    {
        self.entries.push(SyncErrorEntry {
            path,
            error: error.into(),
        });
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn into_result(self) -> Result<()> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(self.into())
        }
    }
}

impl fmt::Display for SyncAggregateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} encountered {} error(s):",
            self.context,
            self.entries.len()
        )?;
        for entry in &self.entries {
            writeln!(f, "- {}: {}", entry.path.display(), entry.error)?;
        }
        Ok(())
    }
}

impl std::error::Error for SyncAggregateError {}

// fn local_has_pending_changes(local: &LocalFileInfo, _inventory: Option<&FileMetadata>) -> bool {
//     !local.is_placeholder() || !local.in_sync() ||

//     // if let (Some(last_modified), Some(entry)) = (local.last_modified, inventory) {
//     //     if let Some(last_modified_secs) = system_time_to_unix_secs(last_modified) {
//     //         return last_modified_secs > entry.updated_at;
//     //     }
//     // }
// }

#[allow(dead_code)]
fn system_time_to_unix_secs(time: SystemTime) -> Option<i64> {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => Some(duration.as_secs() as i64),
        Err(err) => {
            let duration = err.duration();
            Some(-(duration.as_secs() as i64))
        }
    }
}

fn generate_conflict_path(path: &Path) -> PathBuf {
    let timestamp = Utc::now().format("%Y%m%d%H%M%S");
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("item");
    let ext = path.extension().and_then(|value| value.to_str());
    let mut new_name = format!("{}{}_{}", CONFLICT_PREFIX, timestamp, stem);
    if let Some(ext) = ext {
        new_name.push('.');
        new_name.push_str(ext);
    }
    let mut conflict_path = path.to_path_buf();
    conflict_path.set_file_name(new_name);
    conflict_path
}

fn next_child_mode(mode: SyncMode) -> SyncMode {
    match mode {
        SyncMode::FullHierarchy => SyncMode::FullHierarchy,
        SyncMode::PathAndFirstLayer => SyncMode::PathOnly,
        SyncMode::RemoteDelete => SyncMode::PathOnly,
        SyncMode::PathOnly => SyncMode::PathOnly,
    }
}

/// Result of collecting child targets, including pre-fetched remote file info.
struct CollectChildResult {
    /// All child paths (union of local and remote).
    paths: Vec<PathBuf>,
    /// Pre-fetched remote file info keyed by local path.
    remote_files: HashMap<PathBuf, FileResponse>,
}

impl Mount {
    /// Syncs a list of local paths by grouping them under their parent directories.
    pub async fn sync_paths(&self, local_paths: Vec<PathBuf>, mode: SyncMode) -> Result<()> {
        let _sync_guard = self.sync_lock.lock().await;

        if local_paths.is_empty() {
            tracing::debug!(target: "drive::sync", id = %self.id, "No paths provided for sync");
            return Ok(());
        }

        let mut grouped: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

        for path in local_paths {
            let parent = path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| path.clone());
            grouped.entry(parent).or_default().push(path);
        }

        let mut aggregate_error = SyncAggregateError::new(format!("Mount {} sync_paths", self.id));

        for (parent, paths) in grouped.iter() {
            if let Err(err) = self.sync_group(parent, paths, mode, None).await {
                let target_path = paths.first().cloned().unwrap_or_else(|| parent.clone());
                aggregate_error.push(target_path, err);
            }
        }

        drop(_sync_guard);
        aggregate_error.into_result()
    }

    /// Fast path for remote-delete push events: skips `fetch_remote_file_infos`, local/inventory
    /// gathering, and `build_sync_plan` — only runs delete actions (same as `SyncMode::RemoteDelete`
    /// when the target paths are exactly the remote-deleted entries).
    pub async fn apply_remote_deletes_fast(&self, paths: Vec<PathBuf>) -> Result<()> {
        let _sync_guard = self.sync_lock.lock().await;

        if paths.is_empty() {
            return Ok(());
        }

        let mut aggregate_error =
            SyncAggregateError::new(format!("Mount {} apply_remote_deletes_fast", self.id));

        let mut actions: Vec<SyncAction> = Vec::with_capacity(paths.len());
        for path in paths {
            let local = match crate::platform_provider()?
                .virtual_file_ops()
                .query_local_state(&path)
            {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        target: "drive::sync",
                        id = %self.id,
                        path = %path.display(),
                        error = ?e,
                        "apply_remote_deletes_fast: query_local_state failed"
                    );
                    continue;
                }
            };

            if !local.exists {
                tracing::debug!(
                    target: "drive::sync",
                    id = %self.id,
                    path = %path.display(),
                    "apply_remote_deletes_fast: path already absent"
                );
                continue;
            }

            actions.push(SyncAction::DeleteLocalAndInventory {
                path,
                skip_if_not_empty: local.is_directory(),
            });
        }

        if actions.is_empty() {
            return Ok(());
        }

        tracing::info!(
            target: "drive::sync",
            id = %self.id,
            count = actions.len(),
            "apply_remote_deletes_fast"
        );

        self.process_sync_plan_actions_list(&actions, &mut aggregate_error)
            .await?;

        drop(_sync_guard);
        aggregate_error.into_result()
    }

    async fn sync_group(
        &self,
        parent: &PathBuf,
        paths: &[PathBuf],
        mode: SyncMode,
        prefetched_remote_files: Option<HashMap<PathBuf, FileResponse>>,
    ) -> Result<()> {
        tracing::info!(
            target: "drive::sync",
            id = %self.id,
            parent = %parent.display(),
            paths = paths.len(),
            mode = ?mode,
            prefetched = prefetched_remote_files.is_some(),
            "Queued grouped sync"
        );

        let mut aggregate_error = SyncAggregateError::new(format!(
            "Mount {} sync_group({})",
            self.id,
            parent.display()
        ));

        // For sync root, directly walk to descendants
        let sync_root = {
            let config = self.config.read().await;
            config.sync_path.clone()
        };
        if paths.len() == 1 && paths[0] == sync_root {
            tracing::debug!(
                target: "drive::sync",
                id = %self.id,
                parent = %parent.display(),
                "Syncing sync root"
            );
            self.process_walk_requests(
                vec![WalkRequest {
                    path: sync_root,
                    mode,
                    reason: WalkReason::ModePropagation,
                    timing: WalkTiming::Immediate,
                }],
                &mut aggregate_error,
            )
            .await;
            return aggregate_error.into_result();
        }

        let remote_files = match prefetched_remote_files {
            Some(files) => files,
            None => {
                // Remote delete intent should converge quickly without needing remote metadata.
                // We can treat `remote` as missing so the planner will prefer DeleteLocalAndInventory.
                if mode == SyncMode::RemoteDelete {
                    tracing::debug!(
                        target: "drive::sync",
                        id = %self.id,
                        parent = %parent.display(),
                        requested = paths.len(),
                        "Skipping remote metadata fetch for RemoteDelete"
                    );
                    HashMap::new()
                } else {
                    self.fetch_remote_file_infos(parent, paths).await?
                }
            }
        };
        tracing::debug!(
            target: "drive::sync",
            id = %self.id,
            parent = %parent.display(),
            requested = paths.len(),
            fetched = remote_files.len(),
            "Fetched remote metadata for sync group"
        );
        tracing::trace!("{:?}", remote_files);

        let local_files = self.fetch_local_file_infos(paths).await?;
        tracing::debug!(
            target: "drive::sync",
            id = %self.id,
            parent = %parent.display(),
            locals = local_files.len(),
            "Fetched local metadata for sync group"
        );
        tracing::trace!("{:?}", local_files);

        let inventory_files = self.fetch_inventory_entries(paths).await?;
        tracing::trace!("{:?}", inventory_files);

        let plan = self.build_sync_plan(
            parent,
            mode,
            paths,
            &remote_files,
            &local_files,
            &inventory_files,
        );

        tracing::debug!(
            target: "drive::sync",
            id = %self.id,
            parent = %parent.display(),
            actions = plan.actions.len(),
            walks = plan.walk_requests.len(),
            "Planned sync actions"
        );
        tracing::trace!(target: "drive::sync", plan = ?plan, "Planned actions detail");

        let SyncPlan {
            actions,
            walk_requests,
        } = plan;
        let (immediate_walks, deferred_walks): (Vec<_>, Vec<_>) = walk_requests
            .into_iter()
            .partition(|request| request.timing == WalkTiming::Immediate);

        self.process_walk_requests(immediate_walks, &mut aggregate_error)
            .await;

        if let Err(err) = self
            .process_sync_plan_actions_list(&actions, &mut aggregate_error)
            .await
        {
            aggregate_error.push(parent.clone(), err);
        }

        self.process_walk_requests(deferred_walks, &mut aggregate_error)
            .await;
        aggregate_error.into_result()
    }

    async fn process_sync_plan_actions_list(
        &self,
        actions: &[SyncAction],
        aggregate_error: &mut SyncAggregateError,
    ) -> Result<()> {
        let (drive_id, sync_root) = {
            let config = self.config.read().await;
            (Uuid::parse_str(&config.id)?, config.sync_path.clone())
        };

        for action in actions {
            self.process_action(action, &sync_root, &drive_id, aggregate_error)
                .await;
        }

        Ok(())
    }

    fn register_remote_materialization_blockers(&self, path: &Path, is_directory: bool) {
        let is_real_file_mode = is_real_file_sync_mode();
        let create_count = if is_real_file_mode { 2 } else { 1 };
        self.event_blocker.register(
            &EventKind::Create(CreateKind::Any),
            path.to_path_buf(),
            create_count,
        );

        if !is_directory {
            let modify_count = if is_real_file_mode { 3 } else { 1 };
            self.event_blocker.register(
                &EventKind::Modify(ModifyKind::Any),
                path.to_path_buf(),
                modify_count,
            );
        }
    }

    async fn enqueue_download_task(
        &self,
        path: &PathBuf,
        aggregate_error: &mut SyncAggregateError,
    ) {
        let _ = self.task_queue.cancel_by_path(path.clone()).await;

        if let Err(err) = self
            .task_queue
            .enqueue(TaskPayload::download(path.clone()))
            .await
        {
            tracing::error!(
                target: "drive::sync",
                id = %self.id,
                path = %path.display(),
                error = ?err,
                "Failed to enqueue download task"
            );
            aggregate_error.push(path.clone(), anyhow::Error::from(err));
        }
    }

    async fn process_action(
        &self,
        action: &SyncAction,
        sync_root: &PathBuf,
        drive_id: &Uuid,
        aggregate_error: &mut SyncAggregateError,
    ) {
        match action {
            SyncAction::CreatePlaceholderAndInventory { path, remote } => {
                self.register_remote_materialization_blockers(path, remote.file_type == file_type::FOLDER);
                let cr_placeholder =
                    CrPlaceholder::new(path.clone(), sync_root.clone(), drive_id.clone());
                if let Err(err) = cr_placeholder
                    .with_remote_file(remote)
                    .commit(self.inventory.clone())
                {
                    tracing::error!(
                        target: "drive::sync",
                        id = %self.id,
                        path = %path.display(),
                        error = ?err,
                        "Failed to create placeholder and inventory"
                    );
                    aggregate_error.push(path.clone(), err);
                    return;
                }

                if is_real_file_sync_mode()
                    && remote.file_type != file_type::FOLDER
                    && remote.size > 0
                {
                    self.enqueue_download_task(path, aggregate_error).await;
                }
            }
            SyncAction::UpdateInventoryFromRemote {
                path,
                remote,
                invalidate_all,
            } => {
                self.register_remote_materialization_blockers(path, remote.file_type == file_type::FOLDER);
                let cr_placeholder =
                    CrPlaceholder::new(path.clone(), sync_root.clone(), drive_id.clone());
                if let Err(err) = cr_placeholder
                    .with_invalidate_all_range(*invalidate_all)
                    .with_remote_file(remote)
                    .commit(self.inventory.clone())
                {
                    tracing::error!(
                        target: "drive::sync",
                        id = %self.id,
                        path = %path.display(),
                        error = ?err,
                        "Failed to update inventory from remote"
                    );
                    aggregate_error.push(path.clone(), err);
                    return;
                }

                if is_real_file_sync_mode()
                    && remote.file_type != file_type::FOLDER
                    && remote.size > 0
                {
                    self.enqueue_download_task(path, aggregate_error).await;
                }
            }
            SyncAction::QueueUpload { path, reason } => {
                tracing::info!(
                    target: "drive::sync",
                    id = %self.id,
                    path = %path.display(),
                    reason = ?reason,
                    "Queueing upload task"
                );

                if let Err(err) = self
                    .task_queue
                    .enqueue(TaskPayload::upload(path.clone()))
                    .await
                {
                    tracing::error!(
                        target: "drive::sync",
                        id = %self.id,
                        path = %path.display(),
                        error = ?err,
                        "Failed to enqueue upload task"
                    );
                    aggregate_error.push(path.clone(), anyhow::Error::from(err));
                }
            }
            SyncAction::QueueDownload { path, remote:_ } => {
                tracing::info!(
                    target: "drive::sync",
                    id = %self.id,
                    path = %path.display(),
                    "Queueing download task"
                );

                self.register_remote_materialization_blockers(path, false);
                self.enqueue_download_task(path, aggregate_error).await;
            }
            SyncAction::DeleteLocalAndInventory {
                path,
                skip_if_not_empty,
            } => {
                if *skip_if_not_empty {
                    // Check if folder is not empty
                    if let Ok(entries) = std::fs::read_dir(path) {
                        if entries.count() > 0 {
                            tracing::info!(
                                target: "drive::sync",
                                id = %self.id,
                                path = %path.display(),
                                "Folder is empty, skipping deletion"
                            );
                            return;
                        }
                    }
                }

                tracing::info!(
                    target: "drive::sync",
                    id = %self.id,
                    path = %path.display(),
                    "Deleting local file/folder and inventory entry"
                );

                let cr_placeholder =
                    CrPlaceholder::new(path.clone(), sync_root.clone(), drive_id.clone());
                if let Err(err) = cr_placeholder.delete_placeholder(self.inventory.clone()) {
                    tracing::error!(
                        target: "drive::sync",
                        id = %self.id,
                        path = %path.display(),
                        error = ?err,
                        "Failed to delete local file/folder and inventory entry"
                    );
                    aggregate_error.push(path.clone(), anyhow::Error::from(err));
                };
                self.event_blocker
                    .register_once(&EventKind::Remove(RemoveKind::Any), path.clone());
            }
            SyncAction::CreateRemoteFolderIfExist { path } => {
                if !path.exists() {
                    return;
                }
                tracing::info!(
                    target: "drive::sync",
                    id = %self.id,
                    path = %path.display(),
                    "Creating remote folder"
                );
                if let Err(err) = self
                    .task_queue
                    .enqueue(TaskPayload::upload(path.clone()))
                    .await
                {
                    tracing::error!(
                        target: "drive::sync",
                        id = %self.id,
                        path = %path.display(),
                        error = ?err,
                        "Failed to enqueue upload task"
                    );
                    aggregate_error.push(path.clone(), anyhow::Error::from(err));
                }
            }
            SyncAction::RenameLocalWithConflict { original, renamed } => {
                tracing::info!(
                    target: "drive::sync",
                    id = %self.id,
                    original = %original.display(),
                    renamed = %renamed.display(),
                    "Renaming local file to resolve conflict"
                );

                // Cancel tasks for the original path
                _ = self.task_queue.cancel_by_path(original.clone()).await;

                if let Err(err) = std::fs::rename(original, renamed) {
                    tracing::error!(
                        target: "drive::sync",
                        id = %self.id,
                        original = %original.display(),
                        renamed = %renamed.display(),
                        error = ?err,
                        "Failed to rename local file"
                    );
                    aggregate_error.push(original.clone(), anyhow::Error::from(err));
                }
            }
        }
    }

    async fn fetch_local_file_infos(
        &self,
        paths: &[PathBuf],
    ) -> Result<HashMap<PathBuf, VirtualFileState>> {
        if paths.is_empty() {
            return Ok(HashMap::new());
        }

        let targets: Vec<PathBuf> = paths.to_vec();
        let mut entries = HashMap::with_capacity(targets.len());
        let platform = crate::platform_provider()?;
        for path in targets {
            let info = platform.virtual_file_ops().query_local_state(&path)?;
            entries.insert(path, info);
        }

        Ok(entries)
    }

    async fn fetch_remote_file_infos(
        &self,
        parent: &PathBuf,
        paths: &[PathBuf],
    ) -> Result<HashMap<PathBuf, FileResponse>> {
        if paths.is_empty() {
            return Ok(HashMap::new());
        }

        let (remote_base, sync_root) = {
            let config = self.config.read().await;
            (config.remote_path.clone(), config.sync_path.clone())
        };

        let mut target_remote_paths: HashMap<String, PathBuf> = HashMap::with_capacity(paths.len());
        for path in paths {
            let remote_uri =
                local_path_to_cr_uri(path.clone(), sync_root.clone(), remote_base.clone())
                    .with_context(|| format!("failed to map {} to remote uri", path.display()))?;
            target_remote_paths.insert(remote_uri.to_string(), path.clone());
        }

        let parent_remote_uri =
            local_path_to_cr_uri(parent.clone(), sync_root.clone(), remote_base.clone())
                .with_context(|| {
                    format!("failed to map parent {} to remote uri", parent.display())
                })?;
        let parent_uri_str = parent_remote_uri.to_string();

        let mut remote_entries: HashMap<PathBuf, FileResponse> =
            HashMap::with_capacity(paths.len());
        let mut remaining: HashSet<String> = target_remote_paths.keys().cloned().collect();
        let mut previous_response = None;

        while !remaining.is_empty() {
            let response = match self
                .cr_client
                .list_files_all(
                    previous_response.as_ref(),
                    parent_uri_str.as_str(),
                    REMOTE_PAGE_SIZE,
                )
                .await
            {
                Ok(resp) => resp,
                Err(ApiError::ApiError { code, .. })
                    if code == ErrorCode::ParentNotExist as i32 =>
                {
                    tracing::debug!(
                        target: "drive::sync",
                        id = %self.id,
                        parent = %parent.display(),
                        "Remote parent directory missing during fetch"
                    );
                    return Ok(HashMap::new());
                }
                Err(err) => {
                    return Err(err.into());
                }
            };

            for file in &response.res.files {
                if let Some(local_path) = target_remote_paths.get(&file.path) {
                    if remote_entries.contains_key(local_path) {
                        continue;
                    }
                    remote_entries.insert(local_path.clone(), file.clone());
                    remaining.remove(&file.path);
                }
            }

            let has_more = response.more && !remaining.is_empty();
            previous_response = Some(response);

            if !has_more {
                break;
            }
        }

        if !remaining.is_empty() {
            for missing in remaining {
                if let Some(local_path) = target_remote_paths.get(&missing) {
                    tracing::warn!(
                        target: "drive::sync",
                        id = %self.id,
                        path = %local_path.display(),
                        remote_path = %missing,
                        "Remote entry missing during sync"
                    );
                }
            }
        }

        Ok(remote_entries)
    }

    async fn fetch_inventory_entries(
        &self,
        paths: &[PathBuf],
    ) -> Result<HashMap<PathBuf, FileMetadata>> {
        if paths.is_empty() {
            return Ok(HashMap::new());
        }

        let mut targets: Vec<(PathBuf, String)> = Vec::with_capacity(paths.len());
        for path in paths {
            match path.to_str() {
                Some(path_str) => targets.push((path.clone(), path_str.to_string())),
                None => {
                    tracing::warn!(
                        target: "drive::sync",
                        id = %self.id,
                        path = %path.display(),
                        "Unable to convert path to UTF-8 for inventory lookup"
                    );
                }
            }
        }

        if targets.is_empty() {
            return Ok(HashMap::new());
        }

        let inventory = self.inventory.clone();
        let entries = task::spawn_blocking(move || -> Result<HashMap<PathBuf, FileMetadata>> {
            let mut results = HashMap::with_capacity(targets.len());
            for (path_buf, path_str) in targets {
                match inventory.query_by_path(&path_str)? {
                    Some(entry) => {
                        results.insert(path_buf, entry);
                    }
                    None => {}
                }
            }
            Ok(results)
        })
        .await??;

        Ok(entries)
    }

    fn build_sync_plan(
        &self,
        _parent: &PathBuf,
        mode: SyncMode,
        paths: &[PathBuf],
        remote_files: &HashMap<PathBuf, FileResponse>,
        local_files: &HashMap<PathBuf, VirtualFileState>,
        inventory_entries: &HashMap<PathBuf, FileMetadata>,
    ) -> SyncPlan {
        let mut plan = SyncPlan::default();

        for path in paths {
            let local_info = local_files
                .get(path)
                .cloned()
                .unwrap_or_else(VirtualFileState::missing);
            let remote = remote_files.get(path);
            let inventory = inventory_entries.get(path);
            self.plan_entry_actions(path, mode, remote, &local_info, inventory, &mut plan);
        }

        plan
    }

    fn plan_entry_actions(
        &self,
        path: &PathBuf,
        mode: SyncMode,
        remote: Option<&FileResponse>,
        local: &VirtualFileState,
        inventory: Option<&FileMetadata>,
        plan: &mut SyncPlan,
    ) {
        match (remote, local.exists) {
            (Some(remote_entry), true) => self.plan_entry_with_remote_and_local(
                path,
                mode,
                remote_entry,
                local,
                inventory,
                plan,
            ),
            (Some(remote_entry), false) => {
                if skip_none_mode_materialize_zero_byte_remote_file(remote_entry) {
                    tracing::debug!(
                        target: "drive::sync",
                        path = %path.display(),
                        "Skipping local file create: remote size is 0 (wait until upload finishes)"
                    );
                } else {
                    plan.actions
                        .push(SyncAction::CreatePlaceholderAndInventory {
                            path: path.clone(),
                            remote: remote_entry.clone(),
                        });
                }
            }
            (None, true) => {
                self.plan_entry_with_local_only(path, mode, local, inventory, plan);
            }
            (None, false) => {}
        }
    }

    fn plan_entry_with_remote_and_local(
        &self,
        path: &PathBuf,
        mode: SyncMode,
        remote: &FileResponse,
        local: &VirtualFileState,
        inventory: Option<&FileMetadata>,
        plan: &mut SyncPlan,
    ) {
        let remote_is_dir = remote.file_type == file_type::FOLDER;

        if local.is_directory != remote_is_dir {
            if local.is_virtual_placeholder() && local.is_partially_on_disk() {
                plan.actions.push(SyncAction::DeleteLocalAndInventory {
                    path: path.clone(),
                    skip_if_not_empty: false,
                });
            } else {
                let conflict_path = generate_conflict_path(path);
                plan.actions.push(SyncAction::RenameLocalWithConflict {
                    original: path.clone(),
                    renamed: conflict_path,
                });
            }

            if !skip_none_mode_materialize_zero_byte_remote_file(remote) {
                plan.actions
                    .push(SyncAction::CreatePlaceholderAndInventory {
                        path: path.clone(),
                        remote: remote.clone(),
                    });
            } else {
                tracing::debug!(
                    target: "drive::sync",
                    path = %path.display(),
                    "Skipping recreate after type conflict: remote file size is 0"
                );
            }
            return;
        }

        let remote_etag = remote.primary_entity.as_deref().unwrap_or("");
        let etag_match = inventory
            .map(|entry| entry.etag == remote_etag)
            .unwrap_or(false);
        let modify_date_match = inventory
            .and_then(|entry| {
                remote
                    .updated_at
                    .parse::<DateTime<Utc>>()
                    .ok()
                    .map(|updated_at| updated_at.timestamp() == entry.updated_at)
            })
            .unwrap_or(false);

        // Size must match too: during server-side chunked uploads the file can stay at 0 bytes while
        // etag/modified time are unchanged; once upload finishes only `size` updates. Without this,
        // we would skip `plan_file_actions` and never re-hydrate the local copy.
        let size_match = inventory
            .map(|entry| entry.size == remote.size)
            .unwrap_or(false);

        let remote_sz = remote.size.max(0) as u64;
        let disk_needs_hydration = is_real_file_sync_mode()
            && !remote_is_dir
            && remote_sz > 0
            && local
                .file_size
                .map(|s| s != remote_sz)
                .unwrap_or(true);

        if remote_is_dir {
            if !etag_match || !modify_date_match {
                plan.actions.push(SyncAction::UpdateInventoryFromRemote {
                    path: path.clone(),
                    remote: remote.clone(),
                    invalidate_all: false,
                });
            }
            self.maybe_enqueue_walk_for_directory(path, mode, local, false, false, plan);
            return;
        }

        if !etag_match || !modify_date_match || !size_match || disk_needs_hydration {
            self.plan_file_actions(path, remote, local, inventory, plan);
        }
    }

    fn plan_entry_with_local_only(
        &self,
        path: &PathBuf,
        mode: SyncMode,
        local: &VirtualFileState,
        _inventory: Option<&FileMetadata>,
        plan: &mut SyncPlan,
    ) {
        if !local.exists {
            return;
        }

        // Remote delete events should prefer removing local entries instead of re-uploading.
        if mode == SyncMode::RemoteDelete {
            plan.actions.push(SyncAction::DeleteLocalAndInventory {
                path: path.clone(),
                skip_if_not_empty: local.is_directory,
            });
            return;
        }

        if local.is_directory {
            let hydrated = local.has_materialized_children();
            if !hydrated {
                plan.actions.push(SyncAction::DeleteLocalAndInventory {
                    path: path.clone(),
                    skip_if_not_empty: false,
                });
                return;
            }

            self.maybe_enqueue_walk_for_directory(path, mode, local, true, hydrated, plan);
            plan.actions.push(SyncAction::DeleteLocalAndInventory {
                path: path.clone(),
                skip_if_not_empty: true,
            });
            plan.actions
                .push(SyncAction::CreateRemoteFolderIfExist { path: path.clone() });
            return;
        }

        if local.is_virtual_placeholder() && local.is_in_sync() {
            plan.actions.push(SyncAction::DeleteLocalAndInventory {
                path: path.clone(),
                skip_if_not_empty: false,
            });
            return;
        }

        // TODO: search queue if not exist:
        plan.actions.push(SyncAction::QueueUpload {
            path: path.clone(),
            reason: UploadReason::RemoteMissing,
        });
    }

    fn plan_file_actions(
        &self,
        path: &PathBuf,
        remote: &FileResponse,
        local: &VirtualFileState,
        inventory: Option<&FileMetadata>,
        plan: &mut SyncPlan,
    ) {
        if is_real_file_sync_mode() {
            let conflicting =
                inventory.is_some_and(|inv| inv.conflict_state == Some(ConflictState::Pending));
            if conflicting {
                return;
            }
            let remote_sz = remote.size.max(0) as u64;
            let local_sz = local.file_size.unwrap_or(0);
            let inv_sz = inventory.map(|i| i.size.max(0) as u64).unwrap_or(0);
            let pull = local_bytes_match_inventory(local, inventory)
                || (remote.file_type != file_type::FOLDER && remote_sz > local_sz)
                || (remote.file_type != file_type::FOLDER && inv_sz > 0 && local_sz == 0);
            if pull {
                plan.actions.push(SyncAction::QueueDownload {
                    path: path.clone(),
                    remote: remote.clone(),
                });
            } else {
                plan.actions.push(SyncAction::QueueUpload {
                    path: path.clone(),
                    reason: UploadReason::RemoteMismatch,
                });
            }
            return;
        }

        if !local.is_virtual_placeholder() || !local.is_in_sync() {
            let conflicting =
                inventory.is_some_and(|inv| inv.conflict_state == Some(ConflictState::Pending));
            if !conflicting {
                plan.actions.push(SyncAction::QueueUpload {
                    path: path.clone(),
                    reason: UploadReason::RemoteMismatch,
                });
            }
            return;
        }

        let pinned = local.local_availability;
        if pinned == LocalAvailability::AlwaysLocal {
            plan.actions.push(SyncAction::QueueDownload {
                path: path.clone(),
                remote: remote.clone(),
            });
        } else {
            plan.actions.push(SyncAction::UpdateInventoryFromRemote {
                path: path.clone(),
                remote: remote.clone(),
                invalidate_all: !local.is_partially_on_disk(),
            });
        }
    }

    fn maybe_enqueue_walk_for_directory(
        &self,
        path: &PathBuf,
        parent_mode: SyncMode,
        local: &VirtualFileState,
        force_diff: bool,
        immediate: bool,
        plan: &mut SyncPlan,
    ) {
        if !local.is_directory {
            return;
        }

        let timing = if immediate {
            WalkTiming::Immediate
        } else {
            WalkTiming::Deferred
        };

        if matches!(
            parent_mode,
            SyncMode::FullHierarchy | SyncMode::PathAndFirstLayer
        ) && (local.has_materialized_children() || !local.is_virtual_placeholder())
        {
            let mode = next_child_mode(parent_mode);
            self.insert_walk_request(
                path.clone(),
                mode,
                WalkReason::ModePropagation,
                timing,
                plan,
            );
            return;
        }

        if force_diff && parent_mode == SyncMode::PathOnly {
            self.insert_walk_request(
                path.clone(),
                SyncMode::PathOnly,
                WalkReason::DiffTriggered,
                timing,
                plan,
            );
        }
    }

    fn insert_walk_request(
        &self,
        path: PathBuf,
        mode: SyncMode,
        reason: WalkReason,
        timing: WalkTiming,
        plan: &mut SyncPlan,
    ) {
        if plan
            .walk_requests
            .iter()
            .any(|request| request.path == path && request.mode == mode)
        {
            return;
        }

        plan.walk_requests.push(WalkRequest {
            path,
            mode,
            reason,
            timing,
        });
    }

    async fn process_walk_requests(
        &self,
        requests: Vec<WalkRequest>,
        aggregate_error: &mut SyncAggregateError,
    ) {
        for walk in requests {
            match self.collect_child_targets(&walk.path).await {
                Ok(result) => {
                    if result.paths.is_empty() {
                        tracing::trace!(
                            target: "drive::sync",
                            id = %self.id,
                            path = %walk.path.display(),
                            timing = ?walk.timing,
                            "Skipping walk, no children discovered"
                        );
                        continue;
                    }

                    tracing::debug!(
                        target: "drive::sync",
                        id = %self.id,
                        directory = %walk.path.display(),
                        reason = ?walk.reason,
                        next_mode = ?walk.mode,
                        children = result.paths.len(),
                        timing = ?walk.timing,
                        "Walking child directory"
                    );

                    let prefetched = if result.remote_files.is_empty() {
                        None
                    } else {
                        Some(result.remote_files)
                    };
                    let child_future =
                        Box::pin(self.sync_group(&walk.path, &result.paths, walk.mode, prefetched));
                    if let Err(err) = child_future.await {
                        tracing::error!(
                            target: "drive::sync",
                            id = %self.id,
                            directory = %walk.path.display(),
                            error = %err,
                            timing = ?walk.timing,
                            "Failed to walk child directory"
                        );
                        aggregate_error.push(walk.path.clone(), err);
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        target: "drive::sync",
                        id = %self.id,
                        directory = %walk.path.display(),
                        error = %err,
                        timing = ?walk.timing,
                        "Failed to enumerate child directory"
                    );
                    aggregate_error.push(walk.path.clone(), err);
                }
            }
        }
    }

    async fn collect_child_targets(&self, directory: &PathBuf) -> Result<CollectChildResult> {
        let dir_clone = directory.clone();
        let mut children = Vec::new();
        match fs::read_dir(&dir_clone) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    children.push(entry.path());
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).context(format!(
                    "failed to enumerate local directory {}",
                    dir_clone.display()
                ));
            }
        };

        let (remote_children, remote_files) = self.list_remote_children(directory).await?;

        let mut dedup: HashSet<PathBuf> = HashSet::new();
        for child in children.into_iter().chain(remote_children.into_iter()) {
            dedup.insert(child);
        }

        Ok(CollectChildResult {
            paths: dedup.into_iter().collect(),
            remote_files,
        })
    }

    /// Lists remote children and returns both the local paths and the file info map.
    async fn list_remote_children(
        &self,
        directory: &PathBuf,
    ) -> Result<(Vec<PathBuf>, HashMap<PathBuf, FileResponse>)> {
        let (remote_base, sync_root) = {
            let config = self.config.read().await;
            (config.remote_path.clone(), config.sync_path.clone())
        };

        let remote_dir_uri =
            match local_path_to_cr_uri(directory.clone(), sync_root.clone(), remote_base.clone()) {
                Ok(uri) => uri,
                Err(err) => {
                    tracing::warn!(
                        target: "drive::sync",
                        id = %self.id,
                        path = %directory.display(),
                        error = %err,
                        "Failed to map local directory to remote URI while walking"
                    );
                    return Ok((Vec::new(), HashMap::new()));
                }
            };
        let remote_dir_uri_str = remote_dir_uri.to_string();

        let remote_base_uri = match CrUri::new(&remote_base) {
            Ok(uri) => uri,
            Err(err) => {
                tracing::warn!(
                    target: "drive::sync",
                    id = %self.id,
                    remote_base = %remote_base,
                    error = %err,
                    "Failed to parse remote base URI while walking"
                );
                return Ok((Vec::new(), HashMap::new()));
            }
        };

        let mut previous_response = None;
        let mut children = Vec::new();
        let mut remote_files: HashMap<PathBuf, FileResponse> = HashMap::new();

        loop {
            let response = match self
                .cr_client
                .list_files_all(
                    previous_response.as_ref(),
                    remote_dir_uri_str.as_str(),
                    REMOTE_PAGE_SIZE,
                )
                .await
            {
                Ok(resp) => resp,
                Err(ApiError::ApiError { code, .. })
                    if code == ErrorCode::ParentNotExist as i32 =>
                {
                    tracing::debug!(
                        target: "drive::sync",
                        id = %self.id,
                        directory = %directory.display(),
                        "Remote directory missing during walk"
                    );
                    return Ok((Vec::new(), HashMap::new()));
                }
                Err(err) => {
                    return Err(err.into());
                }
            };

            for file in &response.res.files {
                if is_symbolic_link(file) {
                    continue;
                }

                match CrUri::new(&file.path).and_then(|file_uri| {
                    remote_path_to_local_relative_path(&file_uri, &remote_base_uri)
                }) {
                    Ok(relative) => {
                        let mut local_path = sync_root.clone();
                        local_path.push(relative);
                        if local_path
                            .parent()
                            .map(|p| p == directory.as_path())
                            .unwrap_or(false)
                        {
                            children.push(local_path.clone());
                            remote_files.insert(local_path, file.clone());
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            target: "drive::sync",
                            id = %self.id,
                            remote_path = %file.path,
                            error = %err,
                            "Failed to map remote child to local path"
                        );
                    }
                }
            }

            if !response.more {
                break;
            }

            previous_response = Some(response);
        }

        Ok((children, remote_files))
    }
}
