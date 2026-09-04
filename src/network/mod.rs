use core::sync::atomic::{AtomicBool, Ordering};

mod api_client;
mod decoder;
mod places;
mod polling;
mod server;

pub(crate) static ADSB_REQUEST_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) static CONFIGURATION_MODE: AtomicBool = AtomicBool::new(false);

pub fn set_configuration_mode(enabled: bool) {
    CONFIGURATION_MODE.store(enabled, Ordering::Release);
}

pub use polling::start;
