mod traits;
mod types;

pub use traits::{
    AutoStartProvider, DesktopIntegration, FetchDataRequest, FetchDataWriter,
    MountRegistrationContext, MountRegistrationCustomState, MountSession, MountedDriveCallback,
    PlatformMountProvider, PlatformProvider, VirtualFileOps, VirtualFileProvider,
};
pub use types::{
    DesktopIntegrationCapabilities, LocalAvailability, PlaceholderEntry, PlatformCapabilities,
    PlatformKind, VirtualFileMetadata, VirtualFileMode, VirtualFileState, VirtualPlaceholderSpec,
};
