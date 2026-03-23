use crate::shell_host::{DriveStatusUI, ShellExtensionHost, SyncStatus};
use crate::shellext::vector::create_vector;
use crate::utils::app::{AppRoot, get_app_root};
use std::sync::Arc;
use windows::Foundation::{EventRegistrationToken, TypedEventHandler, Uri};
use windows::{
    Storage::Provider::*,
    Win32::{Foundation::*, System::Com::*},
    core::*,
};
// UUID for our custom state handler - matches the C++ implementation
pub const CLSID_STATUS_UI_HANDLER: GUID = GUID::from_u128(0xb1d8ef74_822d_401a_a14a_25f45b1f70b7);

/// Different actions that can be triggered from the Status UI
#[derive(Clone)]
pub enum StatusUIAction {
    /// Show sync status (clicking on the sync status command)
    SyncStatus,
    /// Open user profile URL in browser
    OpenProfile { mount_id: String },
    /// Open storage/capacity details URL in browser
    OpenStorageDetails { mount_id: String },
    /// Open settings window
    OpenSettings,
}

#[implement(IStorageProviderUICommand)]
pub struct SyncStatusUICommand {
     #[allow(dead_code)]
    app_root: AppRoot,
    label: HSTRING,
    description: HSTRING,
    icon: Uri,
    action: StatusUIAction,
    host: Arc<dyn ShellExtensionHost>,
}

impl SyncStatusUICommand {
    pub fn new(
        app_root: AppRoot,
        label: HSTRING,
        description: HSTRING,
        icon: Uri,
        action: StatusUIAction,
        host: Arc<dyn ShellExtensionHost>,
    ) -> Self {
        Self {
            app_root,
            label,
            description,
            icon,
            action,
            host,
        }
    }
}

impl IStorageProviderUICommand_Impl for SyncStatusUICommand_Impl {
    fn Label(&self) -> Result<HSTRING> {
        Ok(self.label.clone())
    }
    fn Description(&self) -> Result<HSTRING> {
        Ok(self.description.clone())
    }
    fn Icon(&self) -> Result<Uri> {
        Ok(self.icon.clone())
    }
    fn State(&self) -> Result<StorageProviderUICommandState> {
        Ok(StorageProviderUICommandState::Enabled)
    }
    fn Invoke(&self) -> Result<()> {
        tracing::debug!(target: "shellext::status_ui", "Invoke called");

        let command = match &self.action {
            StatusUIAction::SyncStatus => {
                tracing::debug!(target: "shellext::status_ui", "SyncStatus action - opening sync status window");
                return self
                    .host
                    .open_sync_status_window()
                    .map_err(|_| Error::from(E_FAIL));
            }
            StatusUIAction::OpenProfile { mount_id } => {
                tracing::debug!(target: "shellext::status_ui", mount_id = %mount_id, "OpenProfile action");
                return self
                    .host
                    .open_profile_url(mount_id)
                    .map_err(|_| Error::from(E_FAIL));
            }
            StatusUIAction::OpenStorageDetails { mount_id } => {
                tracing::debug!(target: "shellext::status_ui", mount_id = %mount_id, "OpenStorageDetails action");
                return self
                    .host
                    .open_storage_details_url(mount_id)
                    .map_err(|_| Error::from(E_FAIL));
            }
            StatusUIAction::OpenSettings => {
                tracing::debug!(target: "shellext::status_ui", "OpenSettings action - opening settings window");
                return self
                    .host
                    .open_settings_window()
                    .map_err(|_| Error::from(E_FAIL));
            }
        };
    }
}

#[implement(IStorageProviderStatusUISource)]
pub struct StatusUIHandler {
    host: Arc<dyn ShellExtensionHost>,
    app_root: AppRoot,
    mount_id: String,
}

impl StatusUIHandler {
    pub fn new(host: Arc<dyn ShellExtensionHost>, mount_id: String) -> Self {
        Self {
            host,
            app_root: get_app_root(),
            mount_id,
        }
    }

    /// Get drive status using the command pattern with blocking_recv
    fn get_drive_status(&self) -> Option<DriveStatusUI> {
        match self.host.get_drive_status_ui(&self.mount_id) {
            Ok(status) => status,
            Err(e) => {
                tracing::error!(target: "shellext::status_ui", error = %e, "Failed to get drive status ui");
                None
            }
        }
    }
}

impl IStorageProviderStatusUISource_Impl for StatusUIHandler_Impl {
    fn GetStatusUI(&self) -> Result<StorageProviderStatusUI> {
        tracing::trace!(target: "shellext::status_ui", mount_id = %self.mount_id, "GetStatusUI");

        let ui = StorageProviderStatusUI::new()?;
        let image_path = self.app_root.image_path();

        // Get drive status from DriveManager
        let drive_status = self.get_drive_status();

        // Set provider state based on sync status
        let (provider_state, _state_label, sync_icon, sync_label, sync_description) = match &drive_status {
            Some(status) => {
                match status.sync_status {
                    SyncStatus::Syncing => (
                        StorageProviderState::Syncing,
                        status.name.clone(),
                        format!("{}\\CloudIconSyncing.svg", image_path),
                        t!("syncing").to_string(),
                        t!("syncingDescription", "count" => status.active_task_count).to_string(),
                    ),
                    SyncStatus::InSync => (
                        StorageProviderState::InSync,
                        status.name.clone(),
                        format!("{}\\CloudIconSynced.svg", image_path),
                        t!("synced").to_string(),
                        t!("syncedDescription").to_string(),
                    ),
                    SyncStatus::Paused => (
                        StorageProviderState::Paused,
                        status.name.clone(),
                        format!("{}\\CloudIconPaused.svg", image_path),
                        t!("paused").to_string(),
                        t!("pausedDescription").to_string(),
                    ),
                    SyncStatus::Error => (
                        StorageProviderState::Error,
                        status.name.clone(),
                        format!("{}\\CloudIconError.svg", image_path),
                        t!("error").to_string(),
                        t!("errorDescription").to_string(),
                    ),
                }
            }
            None => (
                StorageProviderState::InSync,
                "Cloudreve".to_string(),
                format!("{}\\CloudIconSynced.svg", image_path),
                t!("synced").to_string(),
                t!("syncedDescription").to_string(),
            ),
        };

        ui.SetProviderState(provider_state)?;

        ui.SetProviderStateLabel(&HSTRING::from("Cloudreve"))?;
        ui.SetProviderStateIcon(&Uri::CreateUri(&HSTRING::from(format!(
            "{}\\cloudreve.svg",
            image_path
        )))?)?;

        // Set sync status command - clicking shows the sync status window
        let sync_command: IStorageProviderUICommand = SyncStatusUICommand::new(
            self.app_root.clone(),
            HSTRING::from(&sync_label),
            HSTRING::from(&sync_description),
            Uri::CreateUri(&HSTRING::from(&sync_icon))?,
            StatusUIAction::SyncStatus,
            self.host.clone(),
        )
        .into();
        ui.SetSyncStatusCommand(&sync_command)?;

        // Set primary command (capacity details) - only if capacity is available
        if let Some(ref status) = drive_status {
            if let Some(ref capacity) = status.capacity {
                let primary_command: IStorageProviderUICommand = SyncStatusUICommand::new(
                    self.app_root.clone(),
                    HSTRING::from(t!("capacityDetails").to_string()),
                    HSTRING::from(&capacity.label),
                    Uri::CreateUri(&HSTRING::from(format!(
                        "{}\\CloudIconSynced.svg",
                        image_path
                    )))?,
                    StatusUIAction::OpenStorageDetails { mount_id: self.mount_id.clone() },
                    self.host.clone(),
                )
                .into();
                ui.SetProviderPrimaryCommand(&primary_command)?;
            }
        }

        // Set secondary commands (profile and settings) - only if status is available
        if let Some(ref status) = drive_status {
            let profile_command: IStorageProviderUICommand = SyncStatusUICommand::new(
                self.app_root.clone(),
                HSTRING::from(t!("profile").to_string()),
                HSTRING::from(&status.profile_url),
                Uri::CreateUri(&HSTRING::from(format!("{}\\ProfileIcon.svg", image_path)))?,
                StatusUIAction::OpenProfile { mount_id: self.mount_id.clone() },
                self.host.clone(),
            )
            .into();

            let settings_command: IStorageProviderUICommand = SyncStatusUICommand::new(
                self.app_root.clone(),
                HSTRING::from(t!("settings").to_string()),
                HSTRING::from(&status.settings_url),
                Uri::CreateUri(&HSTRING::from(format!("{}\\SettingsIcon.svg", image_path)))?,
                StatusUIAction::OpenSettings,
                self.host.clone(),
            )
            .into();

            let ivector = create_vector::<IStorageProviderUICommand>(vec![
                profile_command.into(),
                settings_command.into(),
            ])?;
            ui.SetProviderSecondaryCommands(&ivector)?;
        }

        // Set quota UI - only if capacity is available
        if let Some(ref status) = drive_status {
            if let Some(ref capacity) = status.capacity {
                let quota_ui = StorageProviderQuotaUI::new()?;
                quota_ui.SetQuotaUsedInBytes(capacity.used as u64)?;
                quota_ui.SetQuotaTotalInBytes(capacity.total as u64)?;
                quota_ui.SetQuotaUsedLabel(&HSTRING::from(&capacity.label))?;
                ui.SetQuotaUI(&quota_ui)?;
            }
        }

        Ok(ui)
    }

    fn StatusUIChanged(
        &self,
        handler: Option<
            &TypedEventHandler<IStorageProviderStatusUISource, windows_core::IInspectable>,
        >,
    ) -> windows_core::Result<EventRegistrationToken> {
        if let Some(handler) = handler {
            let _source: IStorageProviderStatusUISource = unsafe { self.this.cast()? };
            let handler = UIEvent(handler.clone());

            let host = self.host.clone();
            let _ = host.register_status_ui_changed(Arc::new(move || {
                tracing::trace!(target: "shellext::status_ui", "EventRegistrationToken: Invoking status UI changed callback");
                let _ = handler.Invoke(None, None);
            }));
        }
        Ok(EventRegistrationToken::default())
    }

    fn RemoveStatusUIChanged(&self, _token: &EventRegistrationToken) -> windows_core::Result<()> {
        Ok(())
    }
}

#[implement(IStorageProviderStatusUISourceFactory)]
pub struct StatusUIHandlerFactory {
    host: Arc<dyn ShellExtensionHost>,
}

impl StatusUIHandlerFactory {
    pub fn new(host: Arc<dyn ShellExtensionHost>) -> Self {
        Self { host }
    }
}

impl IStorageProviderStatusUISourceFactory_Impl for StatusUIHandlerFactory_Impl {
    fn GetStatusUISource(&self, source_id: &HSTRING) -> Result<IStorageProviderStatusUISource> {
        let mount_id = source_id.to_string();
        tracing::trace!(target: "shellext::status_ui", mount_id = %mount_id, "GetStatusUISource");
        let handler = StatusUIHandler::new(self.host.clone(), mount_id);
        let handler: IStorageProviderStatusUISource = handler.into();
        Ok(handler)
    }
}

struct UIEvent(TypedEventHandler<IStorageProviderStatusUISource, windows_core::IInspectable>);
unsafe impl Send for UIEvent {}

impl UIEvent {
    #[allow(non_snake_case)]
    pub fn Invoke(
        &self,
        source: Option<&IStorageProviderStatusUISource>,
        args: Option<&IInspectable>,
    ) -> windows_core::Result<()> {
        self.0.Invoke(source, args)
    }
}

// Class factory for creating instances of our context menu handler
#[implement(IClassFactory)]
pub struct StatusUIHandlerFactoryFactory {
    host: Arc<dyn ShellExtensionHost>,
}

impl StatusUIHandlerFactoryFactory {
    pub fn new(host: Arc<dyn ShellExtensionHost>) -> Self {
        Self { host }
    }
}

impl IClassFactory_Impl for StatusUIHandlerFactoryFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Option<&IUnknown>,
        iid: *const GUID,
        result: *mut *mut core::ffi::c_void,
    ) -> Result<()> {
        if outer.is_some() {
            return Err(Error::from(CLASS_E_NOAGGREGATION));
        }

        let handler = StatusUIHandlerFactory::new(self.host.clone());
        let handler: IUnknown = handler.into();

        unsafe { handler.query(iid, result).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}
