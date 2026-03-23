pub mod app;
pub mod shell_host;
pub mod shellext;
mod toast;
pub mod utils;
mod virtual_file_ops;

use std::{
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use cloudreve_platforms_api::{
    AutoStartProvider, DesktopIntegration, FetchDataRequest, FetchDataWriter, MountRegistrationContext,
    MountRegistrationCustomState, MountSession, MountedDriveCallback, PlaceholderEntry,
    PlatformCapabilities, PlatformKind, PlatformMountProvider, PlatformProvider, VirtualFileMode,
    VirtualFileOps, VirtualFileProvider,
};
use cloudreve_platforms_windows_cfapi::{
    error::{CResult, CloudErrorKind},
    filter::{
        Request, SyncFilter,
        info,
        ticket,
    },
    metadata::Metadata,
    placeholder_file::PlaceholderFile,
    root::{
        Connection, HydrationType, PopulationType, SecurityId, Session, SyncRootId,
        SyncRootIdBuilder, SyncRootInfo,
    },
    utility::WriteAt,
};
use nt_time::FileTime;
use sha2::{Digest, Sha256};
use url::Url;
use windows::{
    ApplicationModel::{StartupTask, StartupTaskState},
    Storage::Provider::StorageProviderSyncRootManager,
};

#[macro_use]
extern crate rust_i18n;

i18n!("../../../locales");

const STARTUP_TASK_ID: &str = "cloudreve";

#[derive(Debug, Default)]
pub struct WindowsPlatformProvider;

impl WindowsPlatformProvider {
    pub fn new() -> Self {
        Self
    }
}

struct WindowsMountSession(Connection<WindowsCallbackHandler>);

impl MountSession for WindowsMountSession {
    fn disconnect(&self) -> Result<()> {
        self.0.disconnect().context("failed to disconnect sync root")
    }
}

struct TicketWriter(ticket::FetchData);

impl FetchDataWriter for TicketWriter {
    fn write_at(&self, data: &[u8], offset: u64) -> Result<()> {
        self.0
            .write_at(data, offset)
            .map_err(|e| anyhow!("failed to write data at offset {}: {:?}", offset, e))
    }

    fn report_progress(&self, total_bytes: u64, transferred_bytes: u64) -> Result<()> {
        self.0
            .report_progress(total_bytes, transferred_bytes)
            .map_err(|e| anyhow!("failed to report progress: {:?}", e))
    }
}

#[derive(Clone)]
struct WindowsCallbackHandler {
    handler: Arc<dyn MountedDriveCallback>,
}

impl SyncFilter for WindowsCallbackHandler {
    fn fetch_data(
        &self,
        request: Request,
        ticket: ticket::FetchData,
        info: info::FetchData,
    ) -> CResult<()> {
        let req = FetchDataRequest {
            path: request.path().to_path_buf(),
            range: info.required_file_range(),
            writer: Arc::new(TicketWriter(ticket)),
        };

        self.handler
            .fetch_data(req)
            .map_err(|_| CloudErrorKind::Unsuccessful)
    }

    fn deleted(&self, _request: Request, _info: info::Deleted) {}

    fn delete(
        &self,
        _request: Request,
        ticket: ticket::Delete,
        _info: info::Delete,
    ) -> CResult<()> {
        let _ = ticket.pass();
        Ok(())
    }

    fn rename(
        &self,
        request: Request,
        ticket: ticket::Rename,
        info: info::Rename,
    ) -> CResult<()> {
        let src = request.path().to_path_buf();
        let dest = info.target_path().to_path_buf();

        match self.handler.rename(src, dest) {
            Ok(()) => {
                let _ = ticket.pass();
                Ok(())
            }
            Err(_) => Err(CloudErrorKind::Unsuccessful),
        }
    }

    fn fetch_placeholders(
        &self,
        request: Request,
        ticket: ticket::FetchPlaceholders,
        _info: info::FetchPlaceholders,
    ) -> CResult<()> {
        let entries = self
            .handler
            .fetch_placeholders(request.path().to_path_buf())
            .map_err(|_| CloudErrorKind::Unsuccessful)?;

        let mut placeholders = entries
            .iter()
            .filter_map(|entry| placeholder_entry_to_windows(entry).ok())
            .collect::<Vec<PlaceholderFile>>();

        ticket
            .pass_with_placeholder(&mut placeholders)
            .map_err(|_| CloudErrorKind::Unsuccessful)?;

        Ok(())
    }

    fn closed(&self, _request: Request, _info: info::Closed) {}

    fn cancel_fetch_data(&self, _request: Request, _info: info::CancelFetchData) {}

    fn validate_data(
        &self,
        _request: Request,
        _ticket: ticket::ValidateData,
        _info: info::ValidateData,
    ) -> CResult<()> {
        Err(CloudErrorKind::NotSupported)
    }

    fn cancel_fetch_placeholders(&self, _request: Request, _info: info::CancelFetchPlaceholders) {}

    fn opened(&self, _request: Request, _info: info::Opened) {}

    fn dehydrate(
        &self,
        _request: Request,
        _ticket: ticket::Dehydrate,
        _info: info::Dehydrate,
    ) -> CResult<()> {
        Err(CloudErrorKind::NotSupported)
    }

    fn dehydrated(&self, _request: Request, _info: info::Dehydrated) {}

    fn renamed(&self, request: Request, info: info::Renamed) {
        self.handler
            .renamed(info.source_path().to_path_buf(), request.path().to_path_buf());
    }
}

impl PlatformProvider for WindowsPlatformProvider {
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            platform: PlatformKind::Windows,
            virtual_file_mode: self.virtual_files().mode(),
            sync_root_registration: self.is_supported().unwrap_or(false),
            auto_start: true,
            desktop_integration: cloudreve_platforms_api::DesktopIntegrationCapabilities {
                notifications: true,
                status_ui: true,
                file_manager_reveal: true,
            },
        }
    }

    fn mounts(&self) -> &dyn PlatformMountProvider {
        self
    }

    fn virtual_files(&self) -> &dyn VirtualFileProvider {
        self
    }

    fn virtual_file_ops(&self) -> &dyn VirtualFileOps {
        self
    }

    fn desktop_integration(&self) -> &dyn DesktopIntegration {
        self
    }

    fn auto_start(&self) -> &dyn AutoStartProvider {
        self
    }
}

impl PlatformMountProvider for WindowsPlatformProvider {
    fn is_supported(&self) -> Result<bool> {
        StorageProviderSyncRootManager::IsSupported()
            .context("Cloud Filter API is not supported")
            .map_err(Into::into)
    }

    fn ensure_mount_id(
        &self,
        instance_url: &str,
        user_id: &str,
        sync_path: &Path,
    ) -> Result<String> {
        Ok(generate_sync_root_id(instance_url, user_id, sync_path)?.to_os_string().to_string_lossy().to_string())
    }

    fn connect_mount(
        &self,
        context: &MountRegistrationContext,
        handler: Arc<dyn MountedDriveCallback>,
    ) -> Result<Box<dyn MountSession>> {
        if !self.is_supported()? {
            return Err(anyhow!("Cloud Filter API is not supported"));
        }

        let sync_root_id = SyncRootId::from_string(&context.mount_id);

        if !sync_root_id.is_registered()? {
            let mut sync_root_info = SyncRootInfo::default();
            sync_root_info.set_display_name(context.display_name.clone());
            sync_root_info.set_hydration_type(HydrationType::Full);
            sync_root_info.set_population_type(PopulationType::Full);
            if let Some(icon_path) = context.icon_path.as_ref() {
                sync_root_info.set_icon(format!("{icon_path},0"));
            }
            sync_root_info.set_version("1.0.0");
            if let Some(recycle_bin_url) = context.recycle_bin_url.as_ref() {
                sync_root_info
                    .set_recycle_bin_uri(recycle_bin_url)
                    .context("failed to set recycle bin uri")?;
            }
            sync_root_info
                .set_path(&context.sync_path)
                .context("failed to set sync root path")?;
            for MountRegistrationCustomState { id, label } in &context.custom_states {
                sync_root_info.add_custom_state(label, *id as i32)?;
            }
            sync_root_id
                .register(sync_root_info)
                .context("failed to register sync root")?;
        }

        if let Err(e) = sync_root_id.index() {
            tracing::warn!(target: "platforms::windows", error = %e, "Failed to add sync root to search indexer");
        }

        let connection = Session::new()
            .connect(
                &context.sync_path,
                WindowsCallbackHandler { handler },
            )
            .context("failed to connect to sync root")?;

        Ok(Box::new(WindowsMountSession(connection)))
    }

    fn unregister_mount(&self, mount_id: &str) -> Result<()> {
        if mount_id.is_empty() {
            return Ok(());
        }

        SyncRootId::from_string(mount_id)
            .unregister()
            .map_err(|e| anyhow!("Failed to unregister sync root: {}", e))?;

        Ok(())
    }
}

impl VirtualFileProvider for WindowsPlatformProvider {
    fn mode(&self) -> VirtualFileMode {
        VirtualFileMode::Placeholder
    }
}

impl DesktopIntegration for WindowsPlatformProvider {
    fn send_general_text_notification(&self, title: &str, message: &str) {
        toast::send_general_text_toast(title, message);
    }

    fn send_token_expiry_notification(&self, drive_id: &str, title: &str, message: &str) {
        toast::send_token_expiry_toast(drive_id, title, message);
    }

    fn send_conflict_notification(&self, drive_id: &str, path: &Path, inventory_id: i64) {
        toast::send_conflict_toast(drive_id, path, inventory_id);
    }

    fn open_in_file_manager(&self, path: &Path) -> Result<()> {
        showfile::show_path_in_file_manager(path);
        Ok(())
    }
}

impl AutoStartProvider for WindowsPlatformProvider {
    fn is_enabled(&self) -> Result<bool> {
        let task_id: windows::core::HSTRING = STARTUP_TASK_ID.into();
        let task = StartupTask::GetAsync(&task_id)
            .context("Failed to get startup task")?
            .get()
            .context("Failed to get startup task")?;

        let state = task.State().context("Failed to get task state")?;
        Ok(matches!(
            state,
            StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy
        ))
    }

    fn set_enabled(&self, enabled: bool) -> Result<bool> {
        let task_id: windows::core::HSTRING = STARTUP_TASK_ID.into();
        let task = StartupTask::GetAsync(&task_id)
            .context("Failed to get startup task")?
            .get()
            .context("Failed to get startup task")?;

        if enabled {
            let new_state = task
                .RequestEnableAsync()
                .context("Failed to request enable")?
                .get()
                .context("Failed to enable startup task")?;

            Ok(matches!(
                new_state,
                StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy
            ))
        } else {
            task.Disable().context("Failed to disable startup task")?;
            Ok(false)
        }
    }
}

fn placeholder_entry_to_windows(entry: &PlaceholderEntry) -> Result<PlaceholderFile> {
    let created_at = FileTime::from_unix_time(entry.created_unix)?;
    let modified_at = FileTime::from_unix_time(entry.modified_unix)?;
    let metadata = if entry.is_directory {
        Metadata::directory()
    } else {
        Metadata::file()
    }
    .size(entry.size)
    .changed(modified_at)
    .written(modified_at)
    .created(created_at);

    let mut placeholder = PlaceholderFile::new(entry.relative_path.clone()).metadata(metadata);
    if entry.mark_in_sync {
        placeholder = placeholder.mark_in_sync();
    }
    if entry.overwrite {
        placeholder = placeholder.overwrite();
    }
    Ok(placeholder.blob(entry.blob.clone()))
}

fn generate_sync_root_id(instance_url: &str, user_id: &str, sync_path: &Path) -> Result<SyncRootId> {
    let url = Url::parse(instance_url)?;
    let hostname = url
        .host_str()
        .ok_or_else(|| anyhow!("Invalid URL: no host found"))?;

    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hasher.update(sync_path.to_string_lossy().as_bytes());
    let hash_hex = format!("{:x}", hasher.finalize());
    let provider_name = format!("cloudreve{}", &hash_hex[..16]);

    Ok(
        SyncRootIdBuilder::new(provider_name)
            .user_security_id(SecurityId::current_user()?)
            .account_name(user_id)
            .build(),
    )
}
