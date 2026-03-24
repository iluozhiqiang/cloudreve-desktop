use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cloudreve_api::api::ExplorerApi;
use cloudreve_api::models::explorer::{FileURLResponse, FileURLService};
use cloudreve_api::models::uri::{CrUri, CR_URI_PREFIX};
use cloudreve_api::{ApiError, Client};
use cloudreve_platforms_api::VirtualFileState;
use url::Url;

use crate::drive::mounts::DriveConfig;
use crate::inventory::{FileMetadata, InventoryDb};

const MTIME_TOLERANCE_SECS: i64 = 1;

/// Canonical Cloudreve URI from the last [`FileResponse.path`] stored in inventory metadata.
/// Used for delete/move/upload API calls when the URI derived from local path + sync root may not
/// match the server (e.g. shared / collaboration folders, redirects).
pub const INVENTORY_REMOTE_URI_KEY: &str = "sys:desktop_inventory_uri";

/// Resolve the Cloudreve URI for a local path, preferring the last known server `FileResponse.path`
/// from inventory when present.
pub fn cr_uri_for_fs_event_path(
    path: PathBuf,
    sync_root: PathBuf,
    remote_base: String,
    inventory: &InventoryDb,
) -> Result<CrUri> {
    let debug_enabled = tracing::enabled!(target: "drive::utils", tracing::Level::DEBUG);
    if debug_enabled {
        tracing::debug!(
            target: "drive::utils",
            local_path = %path.display(),
            sync_root = %sync_root.display(),
            remote_base = %remote_base,
            "Resolving CR URI for fs event path"
        );
    }

    if let Some(s) = path.to_str() {
        if let Ok(Some(meta)) = inventory.query_by_path(s) {
            if let Some(uri_str) = meta.metadata.get(INVENTORY_REMOTE_URI_KEY) {
                if !uri_str.is_empty() {
                    if debug_enabled {
                        tracing::debug!(
                            target: "drive::utils",
                            local_path = %path.display(),
                            inventory_remote_uri = %uri_str,
                            "Using inventory-stored remote URI for fs event path"
                        );
                    }
                    return CrUri::new(uri_str);
                }
            }
        }
    }

    let uri = local_path_to_cr_uri(path, sync_root, remote_base)?;
    if debug_enabled {
        tracing::debug!(
            target: "drive::utils",
            resolved_remote_uri = %uri.to_string(),
            "Using derived CR URI fallback for fs event path"
        );
    }
    Ok(uri)
}

pub fn local_path_to_cr_uri(path: PathBuf, root: PathBuf, remote_base: String) -> Result<CrUri> {
    let mut base = CrUri::new(&remote_base)?;

    // Strip the root from path to get the relative path
    let relative = path.strip_prefix(&root).context("Path is not under root")?;

    // Convert to string with forward slashes (for URI compatibility)
    let relative_str = relative
        .to_str()
        .context("Path contains invalid UTF-8")?
        .replace("\\", "/");

    // Join the relative path to the base URI if not empty
    if !relative_str.is_empty() {
        base.join(&relative_str.split("/").collect::<Vec<&str>>());
    }

    Ok(base)
}

pub fn remote_path_to_local_relative_path(
    remote_path: &CrUri,
    remote_base: &CrUri,
) -> Result<PathBuf> {
    let remote_path_str = remote_path.path().clone();
    let remote_base_str = remote_base.path().clone();

    // 1. add ending slash if not presented to remote_base_str
    let remote_base_str = if !remote_base_str.ends_with('/') {
        remote_base_str + "/"
    } else {
        remote_base_str
    };

    // 2. remove remote_base_str from remote_path_str
    let relative_path = remote_path_str
        .strip_prefix(&remote_base_str)
        .context("Path is not under remote base")?;

    // 3. make sure OS slash is used
    let relative_path = relative_path.replace("/", std::path::MAIN_SEPARATOR_STR);

    Ok(PathBuf::from(relative_path))
}

/// Map `from` / `to` fields in server file push (SSE) events to a local path under `sync_root`.
///
/// The server may send either a full `cloudreve://...` URI or a slash-led path relative to the
/// subscribed mount. The previous implementation only handled the latter, which breaks
/// **web 删除 → 本地仍残留** when payloads use full URIs.
pub fn local_path_from_remote_file_event_field(
    sync_root: &Path,
    remote_base: &str,
    field: &str,
) -> Result<PathBuf> {
    let field = field.trim();
    if field.is_empty() {
        return Err(anyhow::anyhow!("empty remote file event path"));
    }
    if field.starts_with(CR_URI_PREFIX) {
        let file_uri = CrUri::new(field)?;
        let base_uri = CrUri::new(remote_base)?;
        let rel = remote_path_to_local_relative_path(&file_uri, &base_uri)?;
        Ok(sync_root.join(rel))
    } else {
        let relative_path: PathBuf = field
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        Ok(sync_root.join(relative_path))
    }
}

pub fn local_state_matches_inventory(
    local: &VirtualFileState,
    inventory: Option<&FileMetadata>,
) -> bool {
    let Some(inventory) = inventory else {
        return false;
    };

    if !local.exists || local.is_directory != inventory.is_folder {
        return false;
    }

    if !inventory.is_folder && local.file_size != Some(inventory.size.max(0) as u64) {
        return false;
    }

    let Some(last_modified_unix) = local.last_modified_unix else {
        return false;
    };

    (last_modified_unix - inventory.updated_at).abs() <= MTIME_TOLERANCE_SECS
}

/// Like [`local_state_matches_inventory`] without mtime (used for pull vs upload in real-file sync).
pub fn local_bytes_match_inventory(
    local: &VirtualFileState,
    inventory: Option<&FileMetadata>,
) -> bool {
    let Some(inventory) = inventory else {
        return false;
    };

    if !local.exists || local.is_directory != inventory.is_folder {
        return false;
    }

    if !inventory.is_folder && local.file_size != Some(inventory.size.max(0) as u64) {
        return false;
    }

    true
}

/// Generate a URL to view a folder or file online.
///
/// For folders: pass the folder path as `folder_path` and None for `open_file`
/// For files: pass the parent folder path as `folder_path` and the file path as `open_file`
pub fn view_online_url(
    folder_path: &str,
    open_file: Option<&str>,
    config: &DriveConfig,
) -> Result<String> {
    let mut base = config.instance_url.parse::<Url>()?;
    base.set_path("/home");

    {
        let mut query = base.query_pairs_mut();
        query.append_pair("path", folder_path);

        if let Some(file) = open_file {
            query.append_pair("open", file);
        }

        query.append_pair("user_hint", config.user_id.as_str());
    }

    Ok(base.to_string())
}

pub fn recycle_bin_url(config: &DriveConfig) -> Result<String> {
    let mut base = config.instance_url.parse::<Url>()?;
    base.set_path("/home");

    {
        let mut query = base.query_pairs_mut();
        query.append_pair("user_hint", config.user_id.as_str());
         query.append_pair("path", "cloudreve://trash");
    }

    Ok(base.to_string())
}

/// Request signed download URLs for `uri`. If the server returns a batch error (40081) while an
/// `entity` hint was sent — common when metadata is stale after a file was created on the server —
/// retry once without `entity`.
pub async fn get_file_url_with_entity_fallback(
    client: &Client,
    uri: &str,
    entity: Option<String>,
) -> anyhow::Result<FileURLResponse> {
    let mut request = FileURLService::default();
    request.uris.push(uri.to_string());
    request.entity = entity.clone();

    match client.get_file_url(&request).await {
        Ok(res) => Ok(res),
        Err(e) if entity.is_some() && matches!(&e, ApiError::BatchError { .. }) => {
            tracing::warn!(
                target: "drive::utils",
                uri = %uri,
                error = %e,
                "get_file_url failed with entity hint; retrying without entity"
            );
            let mut retry = FileURLService::default();
            retry.uris.push(uri.to_string());
            client
                .get_file_url(&retry)
                .await
                .map_err(|e| anyhow::anyhow!(e))
        }
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cr_uri_for_fs_event_path, local_bytes_match_inventory, local_path_to_cr_uri,
        local_state_matches_inventory,
    };
    use crate::inventory::{FileMetadata, InventoryDb, MetadataEntry};
    use cloudreve_platforms_api::{LocalAvailability, VirtualFileState};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn sample_state() -> VirtualFileState {
        VirtualFileState {
            exists: true,
            is_directory: false,
            file_size: Some(42),
            last_modified_unix: Some(1_700_000_000),
            is_virtual_placeholder: false,
            in_sync: false,
            partially_on_disk: false,
            local_availability: LocalAvailability::Unspecified,
            children_present: false,
        }
    }

    fn sample_inventory() -> FileMetadata {
        FileMetadata {
            id: 1,
            drive_id: Uuid::nil(),
            is_folder: false,
            local_path: "/tmp/test.txt".to_string(),
            created_at: 1_700_000_000,
            updated_at: 1_700_000_000,
            etag: "etag".to_string(),
            metadata: Default::default(),
            props: None,
            permissions: String::new(),
            shared: false,
            size: 42,
            conflict_state: None,
        }
    }

    #[test]
    fn matches_inventory_with_exact_timestamp() {
        let state = sample_state();
        let inventory = sample_inventory();
        assert!(local_state_matches_inventory(&state, Some(&inventory)));
    }

    #[test]
    fn matches_inventory_with_one_second_tolerance() {
        let state = sample_state();
        let mut inventory = sample_inventory();
        inventory.updated_at += 1;
        assert!(local_state_matches_inventory(&state, Some(&inventory)));
    }

    #[test]
    fn rejects_inventory_when_timestamp_gap_is_too_large() {
        let state = sample_state();
        let mut inventory = sample_inventory();
        inventory.updated_at += 2;
        assert!(!local_state_matches_inventory(&state, Some(&inventory)));
    }

    #[test]
    fn bytes_match_ignores_mtime_for_none_mode_pull_decision() {
        let state = sample_state();
        let mut inventory = sample_inventory();
        inventory.updated_at += 12345;
        assert!(!local_state_matches_inventory(&state, Some(&inventory)));
        assert!(local_bytes_match_inventory(&state, Some(&inventory)));
    }

    #[test]
    fn bytes_match_rejects_when_file_size_differs() {
        let mut state = sample_state();
        state.file_size = Some(0);
        let inventory = sample_inventory();
        assert!(!local_bytes_match_inventory(&state, Some(&inventory)));
    }

    #[test]
    fn ignores_folder_size_but_requires_folder_shape_match() {
        let mut state = sample_state();
        state.is_directory = true;
        state.file_size = Some(999);

        let mut inventory = sample_inventory();
        inventory.is_folder = true;
        inventory.size = 0;

        assert!(local_state_matches_inventory(&state, Some(&inventory)));
    }

    /// When inventory holds the server `FileResponse.path`, delete/move must use it — path-derived
    /// URIs can differ for shared/collaboration roots.
    #[test]
    fn cr_uri_for_delete_prefers_inventory_remote_uri() {
        let dir = tempfile::tempdir().unwrap();
        let db = InventoryDb::with_path(dir.path().join("meta.db")).unwrap();
        let sync_root = dir.path().join("sync");
        std::fs::create_dir_all(&sync_root).unwrap();
        let local_file = sync_root.join("sub").join("f.txt");
        std::fs::create_dir_all(local_file.parent().unwrap()).unwrap();
        let local_str = local_file.to_string_lossy().to_string();

        let remote_base = "cloudreve://my/root".to_string();
        let canonical = "cloudreve://share/collab/actual";

        let mut meta = HashMap::new();
        meta.insert(
            super::INVENTORY_REMOTE_URI_KEY.to_string(),
            canonical.to_string(),
        );

        let entry = MetadataEntry::new(Uuid::new_v4(), local_str, false).with_metadata(meta);
        db.upsert(&entry).unwrap();

        let uri = cr_uri_for_fs_event_path(
            local_file.clone(),
            sync_root.clone(),
            remote_base.clone(),
            &db,
        )
        .unwrap();
        assert_eq!(uri.to_string(), canonical);

        let derived = local_path_to_cr_uri(local_file, sync_root, remote_base).unwrap();
        assert_ne!(derived.to_string(), canonical);
    }

    #[test]
    fn remote_file_event_relative_field_maps_under_sync_root() {
        let sync = PathBuf::from("/tmp/cr-sync-root");
        let p = super::local_path_from_remote_file_event_field(
            &sync,
            "cloudreve://my/mount",
            "/sub/f.txt",
        )
        .unwrap();
        assert_eq!(p, sync.join("sub").join("f.txt"));
    }

    #[test]
    fn remote_file_event_full_uri_maps_under_sync_root() {
        let sync = PathBuf::from("/tmp/cr-sync-root");
        let p = super::local_path_from_remote_file_event_field(
            &sync,
            "cloudreve://my/mount",
            "cloudreve://my/mount/sub/f.txt",
        )
        .unwrap();
        assert_eq!(p, sync.join("sub").join("f.txt"));
    }
}
