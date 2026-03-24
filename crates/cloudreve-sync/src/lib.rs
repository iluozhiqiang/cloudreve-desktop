pub mod config;
pub mod drive;
pub mod events;
pub mod inventory;
pub mod logging;
mod platform_runtime;
pub mod tasks;
pub mod uploader;

// Re-export commonly used types
pub use cloudreve_app_config::{AppConfig, ConfigManager};
pub use drive::manager::{
    CapacitySummary, DriveInfo, DriveInfoStatus, DriveManager, DriveStatusUI, StatusSummary,
    SyncStatus, TaskWithProgress,
};
pub use drive::mounts::{Credentials, DriveConfig};
pub use events::{Event, EventBroadcaster};
pub use logging::{LogConfig, LogGuard};
pub use cloudreve_platforms_api::PlatformCapabilities;
pub use platform_runtime::{is_real_file_sync_mode, platform_provider, set_platform_provider};

/// User agent string for HTTP requests
pub const USER_AGENT: &str = concat!("cloudreve-desktop/", env!("CARGO_PKG_VERSION"));

#[macro_use]
extern crate rust_i18n;

i18n!("../../locales");

