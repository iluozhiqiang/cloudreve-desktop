use super::CrExplorerCommandHandler;
use crate::shell_host::ShellExtensionHost;
use std::sync::Arc;
use windows::{
    Win32::{Foundation::*, System::Com::*},
    core::*,
};

// Class factory for creating instances of our context menu handler
#[implement(IClassFactory)]
pub struct CrExplorerCommandFactory {
    host: Arc<dyn ShellExtensionHost>,
}

impl CrExplorerCommandFactory {
    pub fn new(host: Arc<dyn ShellExtensionHost>) -> Self {
        Self { host }
    }
}

impl IClassFactory_Impl for CrExplorerCommandFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Option<&IUnknown>,
        iid: *const GUID,
        result: *mut *mut core::ffi::c_void,
    ) -> Result<()> {
        if outer.is_some() {
            return Err(Error::from(CLASS_E_NOAGGREGATION));
        }

        let handler = CrExplorerCommandHandler::new(self.host.clone());
        let handler: IUnknown = handler.into();

        unsafe { handler.query(iid, result).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}
