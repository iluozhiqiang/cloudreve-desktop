use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{Context, Result};
use cloudreve_app_config::ConfigManager;
use sha2::{Digest, Sha256};
use cloudreve_platforms_api::{
    AutoStartProvider, DesktopIntegration, LocalAvailability, MountRegistrationContext,
    MountSession, MountedDriveCallback, PlatformCapabilities, PlatformKind, PlatformMountProvider,
    PlatformProvider, VirtualFileMode, VirtualFileOps, VirtualFileState, VirtualPlaceholderSpec,
    VirtualFileProvider,
};
mod file_provider_ipc;
use filetime::{FileTime, set_file_mtime};

const NOTIFICATION_COOLDOWN_SECS: u64 = 15;
const MACOS_AUTOSTART_LABEL: &str = "xyz.cloudreve.desktop";

/// 默认走最简单同步：`VirtualFileMode::None`，不启动 FPE Unix IPC。
/// 需要 File Provider / Finder 集成时再设 `CLOUDREVE_MACOS_FPE=1`（或 `true` / `yes` / `on`）。
fn macos_fpe_ipc_enabled() -> bool {
    match std::env::var("CLOUDREVE_MACOS_FPE") {
        Ok(v) => {
            let v = v.to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes" || v == "on"
        }
        Err(_) => false,
    }
}

fn macos_virtual_file_mode() -> VirtualFileMode {
    if cfg!(target_os = "macos") && macos_fpe_ipc_enabled() {
        VirtualFileMode::FileProvider
    } else {
        VirtualFileMode::None
    }
}

#[derive(Debug, Default)]
pub struct MacosPlatformProvider;

impl MacosPlatformProvider {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Default)]
struct NoopMountSession;

impl MountSession for NoopMountSession {
    fn disconnect(&self) -> Result<()> {
        Ok(())
    }
}

fn hash_mount_id(instance_url: &str, user_id: &str, sync_path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    instance_url.hash(&mut hasher);
    user_id.hash(&mut hasher);
    sync_path.hash(&mut hasher);
    format!("macos-local-{:016x}", hasher.finish())
}

fn apply_modified_time(path: &Path, modified_unix: i64) -> Result<()> {
    let file_time = if modified_unix >= 0 {
        FileTime::from_unix_time(modified_unix, 0)
    } else {
        let duration = Duration::from_secs((-modified_unix) as u64);
        let system_time = UNIX_EPOCH
            .checked_sub(duration)
            .context("failed to compute modified time before unix epoch")?;
        FileTime::from_system_time(system_time)
    };

    set_file_mtime(path, file_time).with_context(|| {
        format!(
            "failed to set file modification time for {}",
            path.display()
        )
    })
}

fn notification_registry() -> &'static Mutex<HashMap<String, Instant>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn should_throttle_notification(key: &str, cooldown: Duration) -> bool {
    let mut registry = notification_registry().lock().unwrap();
    let now = Instant::now();
    if let Some(last_sent) = registry.get(key) {
        if now.duration_since(*last_sent) < cooldown {
            return true;
        }
    }
    registry.insert(key.to_string(), now);
    false
}

fn should_notify_credential_expired() -> bool {
    ConfigManager::try_get()
        .map(|config| config.notify_credential_expired())
        .unwrap_or(true)
}

fn should_notify_file_conflict() -> bool {
    ConfigManager::try_get()
        .map(|config| config.notify_file_conflict())
        .unwrap_or(true)
}

fn notify_with_osascript(title: &str, message: &str, subtitle: Option<&str>) {
    fn escape(value: &str) -> String {
        value.replace('\\', "\\\\").replace('"', "\\\"")
    }

    let mut script = format!(
        "display notification \"{}\" with title \"{}\"",
        escape(message),
        escape(title)
    );

    if let Some(subtitle) = subtitle {
        script.push_str(&format!(" subtitle \"{}\"", escape(subtitle)));
    }

    if let Err(error) = Command::new("osascript").arg("-e").arg(script).output() {
        tracing::debug!(
            target: "platforms::macos",
            error = ?error,
            "Failed to dispatch macOS notification"
        );
    }
}

fn escape_plist(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn launch_agents_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home).join("Library").join("LaunchAgents"))
}

fn launch_agent_plist_path() -> Result<PathBuf> {
    Ok(launch_agents_dir()?.join(format!("{MACOS_AUTOSTART_LABEL}.plist")))
}

fn write_launch_agent_plist(executable: &Path) -> Result<()> {
    let plist_path = launch_agent_plist_path()?;
    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let executable = executable
        .to_str()
        .context("autostart executable path contains invalid UTF-8")?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{program}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>ProcessType</key>
  <string>Interactive</string>
</dict>
</plist>
"#,
        label = MACOS_AUTOSTART_LABEL,
        program = escape_plist(executable),
    );

    fs::write(&plist_path, plist)
        .with_context(|| format!("failed to write {}", plist_path.display()))?;
    Ok(())
}

fn nearest_existing_path(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    loop {
        if current.exists() {
            return Some(current);
        }
        current = current.parent()?.to_path_buf();
    }
}

fn query_children_present(path: &Path) -> bool {
    fs::read_dir(path)
        .ok()
        .and_then(|mut entries| entries.next())
        .is_some()
}

fn placeholder_marker_base_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home)
        .join(".cloudreve")
        .join("macos-file-provider")
        .join("markers"))
}

fn placeholder_marker_path(target_path: &Path) -> Result<PathBuf> {
    let base_dir = placeholder_marker_base_dir()?;
    let input = target_path.to_string_lossy();
    let hash = Sha256::digest(input.as_bytes());
    let hash_hex = format!("{:x}", hash);
    Ok(base_dir.join(format!("{hash_hex}.marker")))
}

fn write_placeholder_marker(target_path: &Path, in_sync: bool) -> Result<()> {
    let marker_path = placeholder_marker_path(target_path)?;
    if let Some(parent) = marker_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(marker_path, if in_sync { b"1" } else { b"0" })?;
    Ok(())
}

fn placeholder_marker_exists(target_path: &Path) -> bool {
    placeholder_marker_path(target_path).map(|p| p.exists()).unwrap_or(false)
}

fn read_placeholder_marker_in_sync(target_path: &Path) -> bool {
    placeholder_marker_path(target_path)
        .ok()
        .and_then(|p| fs::read(p).ok())
        .map(|v| v.first().copied() == Some(b'1'))
        .unwrap_or(false)
}

fn clear_placeholder_marker(target_path: &Path) -> Result<()> {
    if let Ok(marker_path) = placeholder_marker_path(target_path) {
        if marker_path.exists() {
            let _ = fs::remove_file(marker_path);
        }
    }
    Ok(())
}

impl PlatformProvider for MacosPlatformProvider {
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            platform: PlatformKind::Macos,
            virtual_file_mode: macos_virtual_file_mode(),
            sync_root_registration: cfg!(target_os = "macos"),
            auto_start: true,
            desktop_integration: cloudreve_platforms_api::DesktopIntegrationCapabilities {
                notifications: true,
                status_ui: false,
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

impl PlatformMountProvider for MacosPlatformProvider {
    fn is_supported(&self) -> Result<bool> {
        Ok(cfg!(target_os = "macos"))
    }

    fn ensure_mount_id(
        &self,
        instance_url: &str,
        user_id: &str,
        sync_path: &Path,
    ) -> Result<String> {
        Ok(hash_mount_id(instance_url, user_id, sync_path))
    }

    fn connect_mount(
        &self,
        context: &MountRegistrationContext,
        handler: Arc<dyn MountedDriveCallback>,
    ) -> Result<Box<dyn MountSession>> {
        if !macos_fpe_ipc_enabled() {
            tracing::info!(
                target: "platforms::macos",
                mount_id = %context.mount_id,
                "File Provider IPC off (simple sync); set CLOUDREVE_MACOS_FPE=1 to enable"
            );
            let _ = handler;
            return Ok(Box::new(NoopMountSession));
        }

        tracing::info!(
            target: "platforms::macos",
            mount_id = %context.mount_id,
            sync_path = %context.sync_path.display(),
            "Starting File Provider IPC server for mount"
        );

        file_provider_ipc::register_handler_for_mount(context.mount_id.clone(), handler)
    }

    fn unregister_mount(&self, _mount_id: &str) -> Result<()> {
        Ok(())
    }
}

impl VirtualFileProvider for MacosPlatformProvider {
    fn mode(&self) -> VirtualFileMode {
        macos_virtual_file_mode()
    }
}

impl VirtualFileOps for MacosPlatformProvider {
    fn query_local_state(&self, path: &Path) -> Result<VirtualFileState> {
        Ok(match fs::metadata(path) {
            Ok(meta) => {
                let is_dir = meta.is_dir();
                let children_present = is_dir && query_children_present(path);

                let marker_exists = placeholder_marker_exists(path);
                let mut is_virtual_placeholder = marker_exists;

                // If a placeholder marker still exists but local content was actually materialized,
                // best-effort treat it as hydrated and clear the marker.
                if marker_exists && !is_dir && meta.len() > 0 {
                    is_virtual_placeholder = false;
                    let _ = clear_placeholder_marker(path);
                }

                // For directories, treat "placeholder dir" as virtual only when it looks empty.
                if is_virtual_placeholder && is_dir && children_present {
                    is_virtual_placeholder = false;
                }

                // Without a marker, treat as not server-synced (simple sync / plain files).
                let in_sync = if marker_exists {
                    read_placeholder_marker_in_sync(path)
                } else {
                    false
                };

                let local_availability = if is_virtual_placeholder {
                    LocalAvailability::Unspecified
                } else {
                    LocalAvailability::AlwaysLocal
                };

                VirtualFileState {
                    exists: true,
                    is_directory: is_dir,
                    file_size: Some(meta.len()),
                    last_modified_unix: meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64),
                    is_virtual_placeholder,
                    in_sync,
                    partially_on_disk: false,
                    local_availability,
                    children_present,
                }
            }
            Err(_) => VirtualFileState::missing(),
        })
    }

    fn upsert_placeholder(&self, spec: &VirtualPlaceholderSpec) -> Result<()> {
        if let Some(parent) = spec.target_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create parent directory {}", parent.display())
            })?;
        }
        let existing_state = self
            .query_local_state(&spec.target_path)
            .unwrap_or_else(|_| VirtualFileState::missing());

        if spec.is_directory {
            fs::create_dir_all(&spec.target_path).with_context(|| {
                format!("failed to create directory {}", spec.target_path.display())
            })?;
        } else {
            if !spec.target_path.exists() {
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(false)
                    .open(&spec.target_path)
                    .with_context(|| {
                        format!("failed to create file {}", spec.target_path.display())
                    })?;
            }

            // Converting hydrated content into a placeholder is expected when we (re)create
            // the placeholder during remote invalidation / updates.
            if spec.overwrite && existing_state.exists && !existing_state.is_virtual_placeholder {
                OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(&spec.target_path)
                    .with_context(|| {
                        format!("failed to truncate file {}", spec.target_path.display())
                    })?;
            }
        }

        // Always mark as placeholder for File Provider mode, so Finder overlay can distinguish
        // CloudOnly vs Synced.
        write_placeholder_marker(&spec.target_path, spec.mark_in_sync)?;

        apply_modified_time(&spec.target_path, spec.metadata.modified_unix)?;
        Ok(())
    }

    fn remove_placeholder(&self, path: &Path) -> Result<()> {
        let _ = clear_placeholder_marker(path);

        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    fn mark_in_sync(&self, path: &Path, in_sync: bool) -> Result<()> {
        // Ensure marker exists so query_local_state() can reflect the flag.
        if placeholder_marker_exists(path) {
            write_placeholder_marker(path, in_sync)?;
        }
        Ok(())
    }

    fn hydrate_file(&self, _path: &Path, _range: std::ops::Range<u64>) -> Result<()> {
        // When core explicitly requests hydration, drop the placeholder marker so Finder overlay
        // can flip to Synced.
        clear_placeholder_marker(_path)?;
        Ok(())
    }

    fn dehydrate_file(&self, _path: &Path, _range: std::ops::Range<u64>) -> Result<()> {
        // When core explicitly requests dehydrate, restore the placeholder marker (and best-effort
        // truncate local content so query_local_state() doesn't auto-detect as hydrated).
        if _path.exists() && _path.is_file() {
            let _ = OpenOptions::new().write(true).truncate(true).open(_path);
        }
        write_placeholder_marker(_path, true)?;
        Ok(())
    }

    fn set_local_availability(&self, _path: &Path, _availability: LocalAvailability) -> Result<()> {
        match _availability {
            LocalAvailability::AlwaysLocal => {
                clear_placeholder_marker(_path)?;
            }
            LocalAvailability::OnlineOnly => {
                write_placeholder_marker(_path, true)?;
            }
            LocalAvailability::Unspecified => {}
        }
        Ok(())
    }

    fn set_sync_error(&self, _path: &Path, _has_error: bool) -> Result<()> {
        Ok(())
    }
}

impl DesktopIntegration for MacosPlatformProvider {
    fn send_general_text_notification(&self, title: &str, message: &str) {
        if !should_notify_file_conflict()
            || should_throttle_notification(
                &format!("general:{title}:{message}"),
                Duration::from_secs(NOTIFICATION_COOLDOWN_SECS),
            )
        {
            return;
        }
        notify_with_osascript(title, message, None);
    }

    fn send_token_expiry_notification(&self, drive_id: &str, title: &str, message: &str) {
        if !should_notify_credential_expired()
            || should_throttle_notification(
                &format!("token-expiry:{drive_id}"),
                Duration::from_secs(NOTIFICATION_COOLDOWN_SECS),
            )
        {
            return;
        }
        notify_with_osascript(title, message, Some(drive_id));
    }

    fn send_conflict_notification(&self, drive_id: &str, path: &Path, inventory_id: i64) {
        if !should_notify_file_conflict()
            || should_throttle_notification(
                &format!("conflict:{drive_id}:{}:{inventory_id}", path.display()),
                Duration::from_secs(NOTIFICATION_COOLDOWN_SECS),
            )
        {
            return;
        }
        let message = format!("{} ({inventory_id})", path.display());
        notify_with_osascript(drive_id, &message, Some("Conflict"));
    }

    fn open_in_file_manager(&self, path: &Path) -> Result<()> {
        let target = if path.exists() {
            path.to_path_buf()
        } else {
            nearest_existing_path(path)
                .ok_or_else(|| anyhow::anyhow!("No existing parent path for {}", path.display()))?
        };
        showfile::show_path_in_file_manager(&target);
        Ok(())
    }
}

impl AutoStartProvider for MacosPlatformProvider {
    fn is_enabled(&self) -> Result<bool> {
        Ok(launch_agent_plist_path()?.exists())
    }

    fn set_enabled(&self, enabled: bool) -> Result<bool> {
        let plist_path = launch_agent_plist_path()?;
        if enabled {
            let executable = std::env::current_exe().context("failed to get current executable")?;
            write_launch_agent_plist(&executable)?;
            return Ok(true);
        }

        if plist_path.exists() {
            fs::remove_file(&plist_path)
                .with_context(|| format!("failed to remove {}", plist_path.display()))?;
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn ensure_mount_id_is_deterministic() {
        let p = MacosPlatformProvider::new();
        let a = p
            .ensure_mount_id("https://ex.com", "u1", Path::new("/tmp/cr-sync"))
            .unwrap();
        let b = p
            .ensure_mount_id("https://ex.com", "u1", Path::new("/tmp/cr-sync"))
            .unwrap();
        assert_eq!(a, b);
        assert!(a.starts_with("macos-local-"));
    }

    #[test]
    fn ensure_mount_id_changes_with_path() {
        let p = MacosPlatformProvider::new();
        let a = p
            .ensure_mount_id("https://ex.com", "u1", Path::new("/tmp/a"))
            .unwrap();
        let b = p
            .ensure_mount_id("https://ex.com", "u1", Path::new("/tmp/b"))
            .unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn escape_plist_escapes_xml_entities() {
        assert_eq!(escape_plist("A & B"), "A &amp; B");
        assert_eq!(escape_plist("<x>"), "&lt;x&gt;");
        assert_eq!(escape_plist("'q'"), "&apos;q&apos;");
        assert_eq!(escape_plist("\"z\""), "&quot;z&quot;");
    }

    #[test]
    fn notification_throttle_blocks_same_key_within_cooldown() {
        let key = format!(
            "unit-test-throttle-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        assert!(!should_throttle_notification(
            &key,
            Duration::from_secs(60)
        ));
        assert!(should_throttle_notification(
            &key,
            Duration::from_secs(60)
        ));
    }

    #[test]
    fn capabilities_non_virtual_mode_and_autostart() {
        let p = MacosPlatformProvider::new();
        let c = p.capabilities();
        assert_eq!(c.platform, PlatformKind::Macos);
        let expected = macos_virtual_file_mode();
        assert_eq!(c.virtual_file_mode, expected);
        assert!(c.auto_start);
    }
}
