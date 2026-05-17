//! Desktop macOS control bridge.
//!
//! Phase 1 only registers the authorized desktop process as the place where
//! future macOS control calls will run. It exposes permission readiness but
//! deliberately does not implement screen capture, Accessibility snapshots,
//! or input actions yet.

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::Arc;

    use async_trait::async_trait;
    use ha_core::mac_control::MacControlBridge;

    struct TauriMacControlBridge;

    #[async_trait]
    impl MacControlBridge for TauriMacControlBridge {
        async fn system_permissions(&self) -> ha_core::permissions::SystemPermissionsResponse {
            ha_core::permissions::check_system_permissions().await
        }
    }

    pub fn register() {
        let bridge: Arc<dyn MacControlBridge> = Arc::new(TauriMacControlBridge);
        ha_core::mac_control::set_mac_control_bridge(bridge);
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn register() {}
}

pub fn register() {
    imp::register();
}
