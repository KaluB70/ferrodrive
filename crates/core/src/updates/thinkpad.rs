//! ThinkPad, ThinkCentre and ThinkStation models through Lenovo's documented
//! System Update catalog (`download.lenovo.com/catalog/<machine type>_<os>.xml`).

use roxmltree::Node;

use super::{Kind, Update, http, version, xml};
use crate::model::{Device, Scan};

pub const NAME: &str = "Lenovo catalog";

const OPERATING_SYSTEMS: [&str; 2] = ["Win11", "Win10"];
const PACKAGE_DRIVER: &str = "2";
const PACKAGE_BIOS: &str = "3";

/// Looks the machine type (first four characters of the product) up in the
/// catalog; `Ok(None)` when Lenovo publishes no catalog for it.
pub fn search(scan: &Scan) -> Result<Option<Vec<Update>>, String> {
    let Some(machine_type) = scan.system.product.as_deref().and_then(|p| p.get(..4)) else {
        return Ok(None);
    };
    let mut catalog = None;
    for os in OPERATING_SYSTEMS {
        let url = format!("https://download.lenovo.com/catalog/{machine_type}_{os}.xml");
        if let Some(bytes) = http::get_bytes(&url)? {
            catalog = Some(http::decode(&bytes));
            break;
        }
    }
    let Some(catalog) = catalog else {
        return Ok(None);
    };
    let locations = package_locations(&catalog)?;

    // Each package is its own small XML file; fetch them concurrently.
    let packages: Vec<Result<Option<String>, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = locations
            .chunks(locations.len().div_ceil(8).max(1))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|url| http::get_bytes(url).map(|b| b.map(|b| http::decode(&b))))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });

    let mut updates = Vec::new();
    for (location, package) in locations.iter().zip(packages) {
        if let Some(package) = package? {
            updates.extend(package_updates(scan, location, &package)?);
        }
    }
    Ok(Some(updates))
}

fn package_locations(catalog: &str) -> Result<Vec<String>, String> {
    let doc = xml::parse(catalog)?;
    Ok(xml::all(doc.root(), "location")
        .filter_map(|n| n.text())
        .map(|t| t.trim().to_owned())
        .collect())
}

/// Updates one package descriptor offers for this machine.
fn package_updates(scan: &Scan, location: &str, package: &str) -> Result<Vec<Update>, String> {
    let doc = xml::parse(package)?;
    let root = doc.root_element();
    let kind = xml::child(root, "PackageType")
        .and_then(|n| n.attribute("type"))
        .unwrap_or("");
    let version = root.attribute("version").unwrap_or_default();
    let base = Update {
        source: NAME,
        kind: Kind::Driver,
        title: xml::text(root, "Desc")
            .unwrap_or_else(|| root.attribute("name").unwrap_or("").into()),
        version: Some(version.to_owned()),
        date: xml::text(root, "ReleaseDate"),
        url: installer_url(root, location),
        device: None,
        installed: None,
    };

    Ok(match kind {
        PACKAGE_BIOS => bios_update(scan, root, base).into_iter().collect(),
        PACKAGE_DRIVER => driver_updates(scan, root, version, base),
        // Applications and firmware need Lenovo's own detection tools; skip them.
        _ => Vec::new(),
    })
}

/// A BIOS package applies when the installed BIOS matches one of its
/// dependency levels (e.g. `N2HET8*`) and none of its "already installed" levels.
fn bios_update(scan: &Scan, root: Node, base: Update) -> Option<Update> {
    let installed = scan.firmware.version.as_deref()?;
    let levels = |section: &str| -> Vec<String> {
        xml::child(root, section)
            .map(|s| {
                xml::all(s, "Level")
                    .filter_map(|n| n.text())
                    .map(|t| t.trim().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let matches = |pattern: &String| glob_prefix(pattern, installed);
    if !levels("Dependencies").iter().any(matches) || levels("DetectInstall").iter().any(matches) {
        return None;
    }
    Some(Update {
        kind: Kind::Firmware,
        installed: Some(installed.to_owned()),
        ..base
    })
}

/// A driver package applies to devices with one of its hardware ids, when
/// its version is newer than the installed driver.
fn driver_updates(scan: &Scan, root: Node, version: &str, base: Update) -> Vec<Update> {
    let ids: Vec<String> = ["_PnPID", "HardwareID"]
        .iter()
        .flat_map(|name| xml::all(root, name))
        .filter_map(|n| n.text())
        .map(|t| t.trim().to_ascii_uppercase())
        .collect();
    scan.devices
        .iter()
        .filter(|d| {
            d.hardware_ids
                .iter()
                .any(|h| ids.contains(&h.to_ascii_uppercase()))
        })
        .filter_map(|d: &Device| {
            let installed = d.driver.as_ref()?.version.as_deref()?;
            version::newer_than(version, installed)?;
            Some(Update {
                device: Some(d.instance_id.clone()),
                installed: Some(installed.to_owned()),
                ..base.clone()
            })
        })
        .collect()
}

fn installer_url(root: Node, location: &str) -> Option<String> {
    let name = xml::child(root, "Files")
        .and_then(|f| xml::child(f, "Installer"))
        .and_then(|i| xml::text(i, "Name"))?;
    let dir = location.rsplit_once('/')?.0;
    Some(format!("{dir}/{name}"))
}

/// Lenovo levels are prefixes with a trailing `*` (`N2HET8*`).
fn glob_prefix(pattern: &str, value: &str) -> bool {
    let prefix = pattern.trim_end_matches('*').to_ascii_uppercase();
    let value = value.to_ascii_uppercase();
    if pattern.ends_with('*') {
        value.starts_with(&prefix)
    } else {
        value == prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Driver, Firmware};

    const BIOS: &str = include_str!("testdata/thinkpad-bios.xml");
    const DRIVER: &str = include_str!("testdata/thinkpad-driver.xml");
    const LOCATION: &str = "https://download.lenovo.com/pccbbs/mobiles/n2huj60w_2_.xml";

    fn scan(bios: &str) -> Scan {
        Scan {
            firmware: Firmware {
                version: Some(bios.into()),
                ..Default::default()
            },
            devices: vec![Device {
                instance_id: "USB\\VID_17E9&PID_4340&MI_00\\1".into(),
                hardware_ids: vec![
                    "USB\\VID_17E9&PID_4340&REV_3104&MI_00".into(),
                    "USB\\VID_17E9&PID_4340&MI_00".into(),
                ],
                driver: Some(Driver {
                    version: Some("10.1.2700.0".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn offers_bios_only_for_older_matching_levels() {
        let old = package_updates(&scan("N2HET77W (1.60 )"), LOCATION, BIOS).unwrap();
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].kind, Kind::Firmware);
        assert_eq!(
            old[0].url.as_deref(),
            Some("https://download.lenovo.com/pccbbs/mobiles/n2huj60w.exe")
        );
        assert!(
            package_updates(&scan("N2HET85W (1.68 )"), LOCATION, BIOS)
                .unwrap()
                .is_empty()
        );
        assert!(
            package_updates(&scan("R0WET60W"), LOCATION, BIOS)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn offers_driver_for_matching_hardware_id() {
        let updates = package_updates(&scan("x"), LOCATION, DRIVER).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].version.as_deref(), Some("10.1.2875.0"));
        assert_eq!(updates[0].installed.as_deref(), Some("10.1.2700.0"));
    }
}
