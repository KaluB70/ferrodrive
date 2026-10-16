//! Core of Ferrodrive: detects the machine's hardware, installed drivers and
//! firmware so update sources can be matched against it.

pub mod detect;
pub mod model;
pub mod smbios;
pub mod updates;

pub use detect::{Error, scan};
pub use model::{Device, Driver, Firmware, Scan, System};
pub use updates::{Update, check};
