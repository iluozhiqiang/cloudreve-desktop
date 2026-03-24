use crate::shell_host::ShellExtensionHost;
use crate::utils::app::AppRoot;
use rust_i18n::t;
use std::sync::Arc;
use windows::{
    Win32::{Foundation::*, System::Com::*, UI::Shell::*},
    core::*,
};

/// Command that shows "Resolve conflict" menu item for files with pending conflicts
#[implement(IExplorerCommand)]
pub struct ResolveConflictCommandHandler {
    host: Arc<dyn ShellExtensionHost>,
    app_root: AppRoot,
}

impl ResolveConflictCommandHandler {
    pub fn new(host: Arc<dyn ShellExtensionHost>, app_root: AppRoot) -> Self {
        Self {
            host,
            app_root,
        }
    }

    /// Check if the selected file has a pending conflict state
    fn has_pending_conflict(&self, items: Option<&IShellItemArray>) -> bool {
        let Some(items) = items else {
            return false;
        };

        unsafe {
            let count = match items.GetCount() {
                Ok(c) => c,
                Err(_) => return false,
            };

            // Only show for single file selection
            if count != 1 {
                return false;
            }

            let item = match items.GetItemAt(0) {
                Ok(i) => i,
                Err(_) => return false,
            };

            let display_name = match item.GetDisplayName(SIGDN_FILESYSPATH) {
                Ok(d) => d,
                Err(_) => return false,
            };

            let path_str = match display_name.to_string() {
                Ok(s) => s,
                Err(_) => return false,
            };

            self.host
                .get_shell_item_state(std::path::Path::new(&path_str))
                .ok()
                .flatten()
                .map(|state| state.has_pending_conflict)
                .unwrap_or(false)
        }
    }
}

impl IExplorerCommand_Impl for ResolveConflictCommandHandler_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        let title = t!("resolveConflict");
        let hstring = HSTRING::from(title.as_ref());
        unsafe { SHStrDupW(&hstring) }
    }

    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        let icon_path = format!("{}\\conflict1.ico", self.app_root.image_path());
        let hstring = HSTRING::from(icon_path);
        unsafe { SHStrDupW(&hstring) }
    }

    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        Err(Error::from(E_NOTIMPL))
    }

    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(GUID::from_u128(0x7b8f3c21_e5a4_4d89_b612_9f3e8c7a1b54))
    }

    fn GetState(&self, items: Option<&IShellItemArray>, _oktobeslow: BOOL) -> Result<u32> {
        if self.has_pending_conflict(items) {
            Ok(ECS_ENABLED.0 as u32)
        } else {
            Ok(ECS_HIDDEN.0 as u32)
        }
    }

    fn Invoke(
        &self,
        selection: Option<&IShellItemArray>,
        _bindctx: Option<&IBindCtx>,
    ) -> Result<()> {
        tracing::debug!(
            target: "shellext::context_menu",
            "Resolve conflict context menu command invoked"
        );

        let Some(items) = selection else {
            return Ok(());
        };

        unsafe {
            let count = items.GetCount()?;
            if count != 1 {
                return Ok(());
            }

            let item = items.GetItemAt(0)?;
            let display_name = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path_str = display_name.to_string()?;
            let path = std::path::PathBuf::from(&path_str);

            tracing::debug!(
                target: "shellext::context_menu",
                path = %path_str,
                "Opening conflict resolution toast"
            );

            if let Err(e) = self.host.show_conflict_toast(path) {
                tracing::error!(
                    target: "shellext::context_menu",
                    error = %e,
                    "Failed to send ShowConflictToast command"
                );
            }
        }

        Ok(())
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Err(Error::from(E_NOTIMPL))
    }
}
