use serde::Serialize;

/// Everything a scan found on this machine.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Scan {
    pub system: System,
    pub firmware: Firmware,
    pub devices: Vec<Device>,
}

/// Machine identity, used to pick OEM catalogs (Dell, Lenovo, HP, ...).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct System {
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub version: Option<String>,
    pub board_manufacturer: Option<String>,
    pub board_product: Option<String>,
}

/// BIOS / UEFI firmware of the machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Firmware {
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub release_date: Option<String>,
}

/// One hardware device and the driver bound to it, if any.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Device {
    /// OS-specific unique id (PnP instance id on Windows, sysfs path on Linux).
    pub instance_id: String,
    pub name: Option<String>,
    pub class: Option<String>,
    pub manufacturer: Option<String>,
    /// Hardware ids, most specific first, in Windows PnP form
    /// (e.g. `PCI\VEN_8086&DEV_15F3&SUBSYS_00008086&REV_03`) on every OS,
    /// so update sources only need to match one format.
    pub hardware_ids: Vec<String>,
    pub driver: Option<Driver>,
}

/// The installed driver of a device.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Driver {
    pub version: Option<String>,
    /// ISO 8601 date (`YYYY-MM-DD`).
    pub date: Option<String>,
    pub provider: Option<String>,
    /// INF file on Windows, kernel module on Linux.
    pub package: Option<String>,
}
