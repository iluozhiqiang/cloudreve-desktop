use std::{
    ffi::OsString,
    ops::Range,
    path::Path,
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use cloudreve_platforms_api::{
    LocalAvailability, VirtualFileMetadata, VirtualFileOps, VirtualFileState,
    VirtualPlaceholderSpec,
};
use cloudreve_platforms_windows_cfapi::{
    metadata::Metadata,
    placeholder::{ConvertOptions, LocalFileInfo, OpenOptions, PinOptions, PinState, UpdateOptions},
    placeholder_file::PlaceholderFile,
};
use nt_time::FileTime;
use widestring::U16CString;
use windows::{
    Win32::{
        Foundation::E_FAIL,
        Storage::EnhancedStorage::PKEY_LastSyncError,
        System::Variant::VT_UI4,
        UI::Shell::{
            IShellItem2, SHCNE_ATTRIBUTES, SHCNE_CREATE, SHCNE_DELETE, SHCNE_MKDIR, SHCNF_PATHW,
            SHChangeNotify, SHCreateItemFromParsingName,
            PropertiesSystem::{GPS_EXTRINSICPROPERTIESONLY, GPS_READWRITE, IPropertyStore},
        },
    },
    core::PCWSTR,
};
use windows_core::PROPVARIANT;

use crate::WindowsPlatformProvider;

impl VirtualFileOps for WindowsPlatformProvider {
    fn query_local_state(&self, path: &Path) -> Result<VirtualFileState> {
        let info = LocalFileInfo::from_path(path)?;
        Ok(local_file_info_to_state(info))
    }

    fn upsert_placeholder(&self, spec: &VirtualPlaceholderSpec) -> Result<()> {
        let local_state = self.query_local_state(&spec.target_path)?;
        if local_state.exists {
            if !local_state.is_virtual_placeholder {
                let mut local_handle = if local_state.is_directory {
                    OpenOptions::new()
                        .open(&spec.target_path)
                        .context("failed to open local directory")?
                } else {
                    OpenOptions::new()
                        .open_win32(&spec.target_path)
                        .context("failed to open local file")?
                };
                local_handle
                    .convert_to_placeholder(
                        convert_options(spec.identity_blob.as_slice(), spec.mark_in_sync),
                        None,
                    )
                    .context("failed to convert to placeholder")?;
            }

            let mut update_options = UpdateOptions::default()
                .metadata(metadata_to_windows(&spec.metadata, spec.is_directory)?);
            if spec.mark_in_sync {
                update_options = update_options.mark_in_sync();
            }
            if spec.dehydrate_on_update {
                update_options = update_options.dehydrate();
            }
            if spec.has_no_children {
                update_options = update_options.has_no_children();
            }

            let mut local_handle = if spec.dehydrate_on_update {
                OpenOptions::new()
                    .write_access()
                    .exclusive()
                    .open(&spec.target_path)
                    .context("failed to open local placeholder for dehydration")?
            } else if local_state.is_directory {
                OpenOptions::new()
                    .open(&spec.target_path)
                    .context("failed to open local placeholder directory")?
            } else {
                OpenOptions::new()
                    .open_win32(&spec.target_path)
                    .context("failed to open local placeholder file")?
            };
            local_handle
                .update(update_options, None)
                .context("failed to update placeholder")?;
        } else {
            let file_name = spec
                .target_path
                .file_name()
                .context("failed to get file name")?;
            let placeholder = PlaceholderFile::new(file_name)
                .metadata(metadata_to_windows(&spec.metadata, spec.is_directory)?)
                .blob(spec.identity_blob.clone());
            let placeholder = if spec.mark_in_sync {
                placeholder.mark_in_sync()
            } else {
                placeholder
            };
            let placeholder = if spec.overwrite {
                placeholder.overwrite()
            } else {
                placeholder
            };
            let parent_path = spec
                .target_path
                .parent()
                .ok_or_else(|| anyhow!("failed to get parent path"))?;
            placeholder
                .create::<&Path>(parent_path)
                .context("failed to create placeholder")?;
        }

        notify_shell_change(
            &spec.target_path,
            if spec.is_directory {
                SHCNE_MKDIR
            } else {
                SHCNE_CREATE
            },
        )?;

        Ok(())
    }

    fn remove_placeholder(&self, path: &Path) -> Result<()> {
        let state = self.query_local_state(path)?;
        if state.exists {
            if path.is_dir() {
                std::fs::remove_dir_all(path).context("failed to delete local directory")?;
            } else {
                std::fs::remove_file(path).context("failed to delete local file")?;
            }
        }
        notify_shell_change(path, SHCNE_DELETE)?;
        Ok(())
    }

    fn mark_in_sync(&self, path: &Path, in_sync: bool) -> Result<()> {
        let mut retries = 0usize;
        loop {
            match OpenOptions::new()
                .write_access()
                .exclusive()
                .open(path)
            {
                Ok(mut handle) => {
                    handle
                        .mark_in_sync(in_sync, None)
                        .context("failed to mark file in sync")?;
                    return Ok(());
                }
                Err(err) if retries < 5 => {
                    retries += 1;
                    thread::sleep(Duration::from_millis(500 * (1 << (retries - 1))));
                    tracing::warn!(
                        target: "platforms::windows",
                        path = %path.display(),
                        retries,
                        error = ?err,
                        "Retrying mark_in_sync after open failure"
                    );
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    fn hydrate_file(&self, path: &Path, range: Range<u64>) -> Result<()> {
        let mut placeholder = OpenOptions::new()
            .open_win32(path)
            .context("failed to open file for hydration")?;
        placeholder
            .hydrate(range)
            .context("failed to hydrate placeholder")?;
        notify_shell_change(path, SHCNE_ATTRIBUTES)?;
        Ok(())
    }

    fn dehydrate_file(&self, path: &Path, range: Range<u64>) -> Result<()> {
        let mut retries = 0usize;
        loop {
            match OpenOptions::new().open_win32(path) {
                Ok(mut placeholder) => {
                    placeholder
                        .dehydrate(range.clone())
                        .context("failed to dehydrate placeholder")?;
                    notify_shell_change(path, SHCNE_ATTRIBUTES)?;
                    return Ok(());
                }
                Err(err) if retries < 5 => {
                    retries += 1;
                    thread::sleep(Duration::from_millis(500 * (1 << (retries - 1))));
                    tracing::warn!(
                        target: "platforms::windows",
                        path = %path.display(),
                        retries,
                        error = ?err,
                        "Retrying dehydrate after open failure"
                    );
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    fn set_local_availability(&self, path: &Path, availability: LocalAvailability) -> Result<()> {
        let state = match availability {
            LocalAvailability::AlwaysLocal => PinState::Pinned,
            LocalAvailability::OnlineOnly => PinState::Unpinned,
            LocalAvailability::Unspecified => return Ok(()),
        };
        let mut handle = OpenOptions::new()
            .open_win32(path)
            .context("failed to open file for pin state update")?;
        handle
            .mark_pin(state, PinOptions::default())
            .context("failed to update pin state")?;
        notify_shell_change(path, SHCNE_ATTRIBUTES)?;
        Ok(())
    }

    fn set_sync_error(&self, path: &Path, has_error: bool) -> Result<()> {
        let path_wide =
            U16CString::from_os_str(path).context("failed to convert path to wide string")?;

        unsafe {
            let item: IShellItem2 = SHCreateItemFromParsingName(PCWSTR(path_wide.as_ptr()), None)
                .context("failed to create shell item from path")?;
            let property_store: IPropertyStore = item
                .GetPropertyStore(GPS_READWRITE | GPS_EXTRINSICPROPERTIESONLY)
                .context("failed to get property store")?;

            let prop_var = if has_error {
                let mut pv = PROPVARIANT::default().as_raw().clone();
                pv.Anonymous.Anonymous.vt = VT_UI4.0;
                pv.Anonymous.Anonymous.Anonymous.ulVal = E_FAIL.0 as u32;
                PROPVARIANT::from_raw(pv)
            } else {
                PROPVARIANT::from_raw(PROPVARIANT::default().as_raw().clone())
            };

            property_store
                .SetValue(&PKEY_LastSyncError, &prop_var)
                .context("failed to set PKEY_LastSyncError value")?;
            property_store
                .Commit()
                .context("failed to commit property store changes")?;
        }

        Ok(())
    }
}

fn local_file_info_to_state(info: LocalFileInfo) -> VirtualFileState {
    let last_modified_unix = info
        .last_modified
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64);

    VirtualFileState {
        exists: info.exists,
        is_directory: info.is_directory,
        file_size: info.file_size,
        last_modified_unix,
        is_virtual_placeholder: info.is_placeholder(),
        in_sync: info.in_sync(),
        partially_on_disk: info.partial_on_disk(),
        local_availability: match info.pinned() {
            PinState::Pinned => LocalAvailability::AlwaysLocal,
            PinState::Unpinned => LocalAvailability::OnlineOnly,
            PinState::Unspecified => LocalAvailability::Unspecified,
        },
        children_present: info.is_folder_populated(),
    }
}

fn metadata_to_windows(
    metadata: &VirtualFileMetadata,
    is_directory: bool,
) -> Result<Metadata> {
    let created_at = FileTime::from_unix_time(metadata.created_unix)?;
    let modified_at = FileTime::from_unix_time(metadata.modified_unix)?;
    let metadata = if is_directory {
        Metadata::directory()
    } else {
        Metadata::file()
    }
    .size(metadata.size)
    .changed(modified_at)
    .written(modified_at)
    .created(created_at);
    Ok(metadata)
}

fn convert_options(blob: &[u8], mark_in_sync: bool) -> ConvertOptions {
    let options = ConvertOptions::default().blob(blob.to_vec());
    if mark_in_sync {
        options.mark_in_sync()
    } else {
        options
    }
}

fn notify_shell_change(path: &Path, event: windows::Win32::UI::Shell::SHCNE_ID) -> Result<()> {
    let utf16_path = U16CString::from_os_str(path)?;
    unsafe {
        SHChangeNotify(
            event,
            SHCNF_PATHW,
            Some(utf16_path.as_ptr() as *const _),
            None,
        );
    }
    Ok(())
}
