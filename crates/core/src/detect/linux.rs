//! Linux detection through sysfs: DMI for firmware, PCI bus for devices.

use std::fs;
use std::path::Path;

use super::Error;
use crate::model::{Device, Driver, Firmware, Scan, System};

const DMI: &str = "/sys/class/dmi/id";
const PCI_DEVICES: &str = "/sys/bus/pci/devices";

pub fn scan() -> Result<Scan, Error> {
    Ok(Scan {
        system: System {
            manufacturer: dmi("sys_vendor"),
            product: dmi("product_name"),
            version: dmi("product_version"),
            sku: dmi("product_sku"),
            family: dmi("product_family"),
            board_manufacturer: dmi("board_vendor"),
            board_product: dmi("board_name"),
        },
        firmware: Firmware {
            vendor: dmi("bios_vendor"),
            version: dmi("bios_version"),
            release_date: dmi("bios_date"),
        },
        devices: super::timed("devices", pci_devices)?,
    })
}

fn dmi(name: &str) -> Option<String> {
    read_trimmed(&Path::new(DMI).join(name))
}

fn pci_devices() -> Result<Vec<Device>, Error> {
    let entries = match fs::read_dir(PCI_DEVICES) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut devices: Vec<Device> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| pci_device(&entry.path()))
        .collect();
    devices.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    Ok(devices)
}

fn pci_device(path: &Path) -> Option<Device> {
    let hex = |name: &str| {
        read_trimmed(&path.join(name)).map(|v| v.trim_start_matches("0x").to_ascii_uppercase())
    };
    let vendor = hex("vendor")?;
    let device = hex("device")?;
    let revision = hex("revision");
    let subsystem = hex("subsystem_device").zip(hex("subsystem_vendor"));

    let base = format!("PCI\\VEN_{vendor}&DEV_{device}");
    let mut hardware_ids = Vec::with_capacity(4);
    if let Some((sub_device, sub_vendor)) = &subsystem {
        let with_subsys = format!("{base}&SUBSYS_{sub_device}{sub_vendor}");
        if let Some(rev) = revision.as_deref().map(|r| &r[r.len().saturating_sub(2)..]) {
            hardware_ids.push(format!("{with_subsys}&REV_{rev}"));
        }
        hardware_ids.push(with_subsys);
    }
    hardware_ids.push(base);

    let module = fs::read_link(path.join("driver/module"))
        .ok()
        .and_then(|link| link.file_name().map(|n| n.to_string_lossy().into_owned()));
    let driver = fs::read_link(path.join("driver")).ok().map(|_| Driver {
        version: module
            .as_deref()
            .and_then(|m| read_trimmed(&Path::new("/sys/module").join(m).join("version"))),
        date: None,
        provider: None,
        package: module.clone(),
    });

    Some(Device {
        instance_id: path.file_name()?.to_string_lossy().into_owned(),
        name: None,
        class: hex("class").map(|c| pci_class_name(&c).to_owned()),
        manufacturer: None,
        hardware_ids,
        driver,
    })
}

/// Maps the PCI base class (first byte of the 24-bit class code) to a name.
fn pci_class_name(class: &str) -> &'static str {
    match class.get(..2) {
        Some("01") => "Storage",
        Some("02") => "Network",
        Some("03") => "Display",
        Some("04") => "Multimedia",
        Some("06") => "Bridge",
        Some("0C") => "Serial bus",
        Some("0D") => "Wireless",
        _ => "Other",
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}
