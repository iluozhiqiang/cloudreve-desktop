use crate::shell_host::ShellExtensionHost;
use crate::utils::app::{AppRoot, get_app_root};
use std::sync::Arc;
use windows::{
    Foundation::Collections::*,
    Storage::Provider::*,
    Win32::{Foundation::*, System::Com::*},
    core::*,
};

// UUID for our custom state handler - matches the C++ implementation
pub const CLSID_CUSTOM_STATE_HANDLER: GUID =
    GUID::from_u128(0xf0c9de6c_6c76_44d7_a58e_579cdf7af263);

#[implement(IStorageProviderItemPropertySource)]
pub struct CustomStateHandler {
    host: Arc<dyn ShellExtensionHost>,
    app_root: AppRoot,
}

impl CustomStateHandler {
    pub fn new(host: Arc<dyn ShellExtensionHost>) -> Self {
        Self {
            host,
            app_root: get_app_root(),
        }
    }
}

impl IStorageProviderItemPropertySource_Impl for CustomStateHandler_Impl {
    fn GetItemProperties(
        &self,
        itempath: &HSTRING,
    ) -> Result<IIterable<StorageProviderItemProperty>> {
        tracing::info!(target: "shellext::custom_state", "Getting item properties for {}", itempath);

        let item_state = self
            .host
            .get_shell_item_state(std::path::Path::new(itempath.to_string().as_str()))
            .map_err(|e| {
                tracing::error!(target: "shellext::custom_state", "Failed to query inventory for path {}: {:?}", itempath, e);
                Error::from(E_FAIL)
            })?
            .ok_or_else(|| {
                tracing::error!(target: "shellext::custom_state", "No metadata found for path {}", itempath);
                Error::from(E_FAIL)
            })?;

        let image_path = self.app_root.image_path();
        let mut vec = Vec::new();

        if item_state.shared {
            let properties = StorageProviderItemProperty::new()?;
            properties.SetId(1)?;
            properties.SetIconResource(&HSTRING::from(format!("{}\\people.ico,0", image_path)))?;
            properties.SetValue(&HSTRING::from(t!("shared").as_ref()))?;
            vec.push(Some(properties));
        }

        if !item_state.readable {
            let properties = StorageProviderItemProperty::new()?;
            properties.SetId(2)?;
            properties
                .SetIconResource(&HSTRING::from(format!("{}\\lock.ico,0", image_path)))?;
            properties.SetValue(&HSTRING::from(t!("noAccess").as_ref()))?;
            vec.push(Some(properties));
        }

        IIterable::<StorageProviderItemProperty>::try_from(vec)
    }
}

// Class factory for creating instances of our context menu handler
#[implement(IClassFactory)]
pub struct CustomStateHandlerFactory {
    host: Arc<dyn ShellExtensionHost>,
}

impl CustomStateHandlerFactory {
    pub fn new(host: Arc<dyn ShellExtensionHost>) -> Self {
        Self { host }
    }
}

impl IClassFactory_Impl for CustomStateHandlerFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Option<&IUnknown>,
        iid: *const GUID,
        result: *mut *mut core::ffi::c_void,
    ) -> Result<()> {
        if outer.is_some() {
            return Err(Error::from(CLASS_E_NOAGGREGATION));
        }

        let handler = CustomStateHandler::new(self.host.clone());
        let handler: IUnknown = handler.into();

        unsafe { handler.query(iid, result).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}
