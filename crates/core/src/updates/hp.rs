//! HP business models through the HP Image Assistant reference files
//! (`hpia.hpcloud.hp.com`), which HP publishes per platform and Windows release.

use std::collections::HashMap;

use roxmltree::Node;

use super::{Kind, Update, http, version, xml};
use crate::model::Scan;

pub const NAME: &str = "HP catalog";

const BASE: &str = "https://hpia.hpcloud.hp.com/ref";

pub fn applies(scan: &Scan) -> bool {
    let maker = scan
        .system
        .manufacturer
        .as_deref()
        .unwrap_or_default()
        .to_ascii_uppercase();
    (maker == "HP" || maker.starts_with("HEWLETT")) && platform_id(scan).is_some()
}

/// HP's platform (system) id is the baseboard product, e.g. `8549`.
fn platform_id(scan: &Scan) -> Option<&str> {
    scan.system
        .board_product
        .as_deref()
        .filter(|p| p.len() == 4)
}

pub fn search(scan: &Scan) -> Result<Vec<Update>, String> {
    let platform = platform_id(scan).unwrap_or_default();
    let list =
        http::get_cab(&format!("{BASE}/platformList.cab"))?.ok_or("platform list missing")?;
    let Some(file) = reference_file(&list, platform)? else {
        return Err(format!(
            "HP publishes no reference file for platform {platform}"
        ));
    };
    let reference = http::get_cab(&format!("{BASE}/{platform}/{file}"))?
        .ok_or_else(|| format!("reference file {file} missing"))?;
    updates(scan, &reference)
}

/// Name of the newest 64-bit reference file for `platform` in the platform list.
fn reference_file(list: &str, platform: &str) -> Result<Option<String>, String> {
    let doc = xml::parse(list)?;
    let newest = xml::all(doc.root(), "Platform")
        .filter(|p| {
            xml::child_text(*p, "SystemID").is_some_and(|id| id.eq_ignore_ascii_case(platform))
        })
        .flat_map(|p| xml::all(p, "OS").collect::<Vec<_>>())
        .filter(|os| xml::child_text(*os, "OSArchitecture").as_deref() == Some("64"))
        .filter_map(|os| {
            let build: u32 = xml::child_text(os, "OSBuildId")?.parse().ok()?;
            // Windows 11 entries say OSVersion 10.0; the file name says 11.0.
            let windows_11 = xml::child_text(os, "IsWindows11").as_deref() == Some("true");
            let version = if windows_11 {
                "11.0".to_owned()
            } else {
                xml::child_text(os, "OSVersion")?
            };
            let release = xml::child_text(os, "OSReleaseIdFilename")?;
            Some((
                build,
                format!(
                    "{platform}_64_{version}.{}.cab",
                    release.to_ascii_lowercase()
                ),
            ))
        })
        .max_by_key(|(build, _)| *build);
    Ok(newest.map(|(_, file)| file.to_ascii_lowercase()))
}

fn updates(scan: &Scan, reference: &str) -> Result<Vec<Update>, String> {
    let doc = xml::parse(reference)?;
    let softpaqs: HashMap<String, Node> = xml::all(doc.root(), "UpdateInfo")
        .filter_map(|n| Some((xml::child_text(n, "Id")?, n)))
        .collect();
    let softpaq_for = |node: Node| {
        let id = xml::child(node, "Solutions")
            .and_then(|s| xml::child(s, "UpdateInfo"))
            .and_then(|u| u.attribute("IdRef"))?;
        softpaqs.get(id).copied()
    };

    let mut updates = Vec::new();

    // BIOS: the reference's own system entry points at the current BIOS softpaq.
    if let (Some(system), Some(installed)) = (
        xml::find(doc.root(), "System"),
        scan.firmware.version.as_deref(),
    ) {
        if let Some(softpaq) = softpaq_for(system) {
            let offered = xml::child_text(softpaq, "Version").unwrap_or_default();
            let current = version::dotted(installed)
                .into_iter()
                .next()
                .unwrap_or(installed);
            if version::compare(&offered, current) == std::cmp::Ordering::Greater {
                updates.push(update(softpaq, Kind::Firmware, None, installed));
            }
        }
    }

    // Drivers: each reference device lists the driver version its softpaq installs.
    for reference_device in xml::all(doc.root(), "Device") {
        let (Some(id), Some(offered)) = (
            xml::child_text(reference_device, "DeviceId"),
            xml::child_text(reference_device, "DriverVersion"),
        ) else {
            continue;
        };
        let Some(device) = super::device_with_hardware_id(scan, &id) else {
            continue;
        };
        let Some(installed) = device.driver.as_ref().and_then(|d| d.version.as_deref()) else {
            continue;
        };
        if version::compare(&offered, installed) != std::cmp::Ordering::Greater {
            continue;
        }
        if let Some(softpaq) = softpaq_for(reference_device) {
            let mut update = update(softpaq, Kind::Driver, Some(&device.instance_id), installed);
            update.version = Some(offered);
            updates.push(update);
        }
    }
    Ok(updates)
}

fn update(softpaq: Node, kind: Kind, device: Option<&str>, installed: &str) -> Update {
    Update {
        source: NAME,
        kind,
        title: xml::child_text(softpaq, "Name").unwrap_or_default(),
        version: xml::child_text(softpaq, "Version"),
        date: xml::child_text(softpaq, "DateReleased"),
        url: xml::child_text(softpaq, "Url").map(|u| format!("https://{u}")),
        device: device.map(str::to_owned),
        installed: Some(installed.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Device, Driver, Firmware, System};

    const LIST: &str = include_str!("testdata/hp-platformlist.xml");
    const REFERENCE: &str = include_str!("testdata/hp-8549.xml");

    #[test]
    fn picks_newest_reference_file() {
        assert_eq!(
            reference_file(LIST, "8549").unwrap().as_deref(),
            Some("8549_64_11.0.24h2.cab")
        );
        assert_eq!(reference_file(LIST, "0000").unwrap(), None);
    }

    #[test]
    fn offers_newer_bios_and_driver() {
        let scan = Scan {
            system: System {
                manufacturer: Some("HP".into()),
                board_product: Some("8549".into()),
                ..Default::default()
            },
            firmware: Firmware {
                version: Some("R70 Ver. 01.30.00".into()),
                ..Default::default()
            },
            devices: vec![Device {
                instance_id: "ACPI\\HPQ6001\\0".into(),
                hardware_ids: vec!["ACPI\\HPQ6001".into()],
                driver: Some(Driver {
                    version: Some("2.1.16.0".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }],
        };
        let updates = updates(&scan, REFERENCE).unwrap();
        let summary: Vec<_> = updates
            .iter()
            .map(|u| (u.kind, u.version.as_deref(), u.title.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    Kind::Firmware,
                    Some("01.36.00"),
                    "HP BIOS and System Firmware (R70)"
                ),
                (Kind::Driver, Some("2.1.17.7"), "HP Wireless Button Driver"),
            ]
        );
        assert_eq!(
            updates[0].url.as_deref(),
            Some("https://ftp.hp.com/pub/softpaq/sp174001-174500/sp174025.exe")
        );
    }
}
