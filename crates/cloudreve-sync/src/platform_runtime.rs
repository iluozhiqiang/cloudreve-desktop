use std::sync::{Arc, OnceLock};

use anyhow::{Result, anyhow};
use cloudreve_platforms_api::PlatformProvider;

static PLATFORM_PROVIDER: OnceLock<Arc<dyn PlatformProvider>> = OnceLock::new();

pub fn set_platform_provider(provider: Arc<dyn PlatformProvider>) -> Result<()> {
    PLATFORM_PROVIDER
        .set(provider)
        .map_err(|_| anyhow!("platform provider already initialized"))
}

pub fn platform_provider() -> Result<Arc<dyn PlatformProvider>> {
    PLATFORM_PROVIDER
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("platform provider is not initialized"))
}
