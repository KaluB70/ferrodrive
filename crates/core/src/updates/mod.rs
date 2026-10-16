//! Update sources: where newer drivers and firmware are looked up.

use serde::Serialize;

use crate::detect::Error;

#[cfg(windows)]
mod windows_update;

/// A driver or firmware update offered by a source.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Update {
    pub source: &'static str,
    pub title: String,
    /// Hardware id the update was published for, in Windows PnP form.
    pub hardware_id: Option<String>,
    /// ISO 8601 date (`YYYY-MM-DD`) of the offered driver.
    pub driver_date: Option<String>,
    pub provider: Option<String>,
    pub class: Option<String>,
}

/// Asks every available source for updates applicable to this machine.
pub fn check() -> Result<Vec<Update>, Error> {
    #[cfg(windows)]
    return super::detect::timed("windows update", windows_update::search);
    #[cfg(not(windows))]
    Err(Error::Unsupported)
}
