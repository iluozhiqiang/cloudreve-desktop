use std::{path::PathBuf, sync::Arc};

use crate::{
    drive::{
        commands::GetPlacehodlerResult,
        commands::MountCommand,
        sync::{
            cloud_file_to_metadata_entry, cloud_file_to_placeholder_entry_merged, is_symbolic_link,
        },
    },
    inventory::{InventoryDb, MetadataEntry},
};
use anyhow::{Context, Result};
use cloudreve_platforms_api::{
    FetchDataRequest, FileProviderItemState, MountedDriveCallback, PlaceholderEntry,
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

#[derive(Clone)]
pub struct MountedDriveCallbackAdapter {
    command_tx: mpsc::UnboundedSender<MountCommand>,
    id: String,
    inventory: Arc<InventoryDb>,
}

impl MountedDriveCallbackAdapter {
    pub fn new(
        command_tx: mpsc::UnboundedSender<MountCommand>,
        id: String,
        inventory: Arc<InventoryDb>,
    ) -> Self {
        Self {
            command_tx,
            id: id,
            inventory: inventory,
        }
    }
}

impl MountedDriveCallback for MountedDriveCallbackAdapter {
    fn fetch_data(&self, request: FetchDataRequest) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = MountCommand::FetchData {
            request,
            response: response_tx,
        };
        self.command_tx
            .send(command)
            .context("Failed to send FetchData command")?;

        response_rx
            .blocking_recv()
            .context("FetchData response channel closed")?
    }

    fn fetch_placeholders(&self, path: PathBuf) -> Result<Vec<PlaceholderEntry>> {
        let files = self.fetch_placeholder_result(path)?;
        Ok(self.build_placeholder_entries(&files))
    }

    fn get_item_state(&self, path: PathBuf) -> Result<FileProviderItemState> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = MountCommand::GetItemState {
            path,
            response: response_tx,
        };
        self.command_tx
            .send(command)
            .context("Failed to send GetItemState command")?;

        response_rx
            .blocking_recv()
            .context("GetItemState response channel closed")?
    }

    fn rename(&self, source: PathBuf, target: PathBuf) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = MountCommand::Rename {
            source,
            target,
            response: response_tx,
        };
        self.command_tx
            .send(command)
            .context("Failed to send rename command")?;

        response_rx
            .blocking_recv()
            .context("Rename response channel closed")?
    }

    fn renamed(&self, source: PathBuf, destination: PathBuf) {
        let command = MountCommand::Renamed {
            source,
            destination,
        };
        if let Err(e) = self.command_tx.send(command) {
            tracing::error!(target: "drive::mounts", id = %self.id, error = %e, "Failed to send Renamed command");
        }
    }
}

impl MountedDriveCallbackAdapter {
    fn fetch_placeholder_result(&self, path: PathBuf) -> Result<GetPlacehodlerResult> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = MountCommand::FetchPlaceholders {
            path,
            response: response_tx,
        };
        self.command_tx
            .send(command)
            .context("Failed to send FetchPlaceholders command")?;

        let files = response_rx
            .blocking_recv()
            .context("FetchPlaceholders response channel closed")??;
        self.insert_placeholder_inventory(&files);
        Ok(files)
    }

    fn build_placeholder_entries(&self, files: &GetPlacehodlerResult) -> Vec<PlaceholderEntry> {
        files
            .files
            .iter()
            .filter(|file| !is_symbolic_link(file))
            .filter_map(|file| {
                let mut local_path = files.local_path.clone();
                local_path.push(file.name.clone());
                let path_str = local_path.to_str()?;
                let inv = self.inventory.query_by_path(path_str).ok().flatten();
                cloud_file_to_placeholder_entry_merged(file, &files.remote_path, inv.as_ref())
                    .map_err(|e| {
                        tracing::error!(
                            target: "drive::mounts",
                            id = %self.id,
                            error = %e,
                            "Failed to convert cloud file to placeholder entry"
                        );
                    })
                    .ok()
            })
            .collect()
    }

    fn insert_placeholder_inventory(&self, files: &GetPlacehodlerResult) {
        let drive_id = Uuid::parse_str(&self.id).unwrap_or_else(|e| {
            tracing::error!(target: "drive::mounts", id = %self.id, error = %e, "Failed to parse drive ID");
            Uuid::new_v4()
        });
        let entries = files
            .files
            .iter()
            .filter_map(|f| {
                let mut entry = cloud_file_to_metadata_entry(f, &drive_id, &files.local_path)
                    .map_err(|e| {
                        tracing::error!(
                            target: "drive::mounts",
                            id = %self.id,
                            error = %e,
                            "Failed to convert cloud file to metadata entry"
                        );
                    })
                    .ok()?;
                if !entry.is_folder {
                    if let Ok(Some(meta)) = self.inventory.query_by_path(&entry.local_path) {
                        entry.size = entry.size.max(meta.size);
                    }
                }
                Some(entry)
            })
            .collect::<Vec<MetadataEntry>>();
        if let Err(e) = self.inventory.batch_insert(&entries) {
            tracing::error!(target: "drive::mounts", id = %self.id, error = ?e, "Failed to insert placeholders into inventory");
        }
    }
}
