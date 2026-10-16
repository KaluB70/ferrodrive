//! Dell client models through Dell's documented update catalogs
//! (`downloads.dell.com/catalog`), one catalog per system id.

use roxmltree::Node;

use super::{Kind, Update, http, version, xml};
use crate::model::Scan;

pub const NAME: &str = "Dell catalog";

const BASE: &str = "https://downloads.dell.com";

pub fn applies(scan: &Scan) -> bool {
    let maker = scan.system.manufacturer.as_deref().unwrap_or_default();
    maker.to_ascii_uppercase().starts_with("DELL") && system_id(scan).is_some()
}

/// Dell's system id is the four-hex-digit SMBIOS SKU number, e.g. `0A3D`.
fn system_id(scan: &Scan) -> Option<&str> {
    scan.system
        .sku
        .as_deref()
        .filter(|s| s.len() == 4 && s.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn search(scan: &Scan) -> Result<Vec<Update>, String> {
    let id = system_id(scan).unwrap_or_default();
    let index = http::get_cab(&format!("{BASE}/catalog/CatalogIndexPC.cab"))?
        .ok_or("catalog index missing")?;
    let Some(path) = model_catalog_path(&index, id)? else {
        return Err(format!("Dell publishes no catalog for system id {id}"));
    };
    let catalog =
        http::get_cab(&format!("{BASE}/{path}"))?.ok_or_else(|| format!("{path} missing"))?;
    updates(scan, &catalog)
}

fn model_catalog_path(index: &str, system_id: &str) -> Result<Option<String>, String> {
    let doc = xml::parse(index)?;
    Ok(xml::all(doc.root(), "GroupManifest")
        .find(|g| {
            xml::all(*g, "Model").any(|m| {
                m.attribute("systemID")
                    .is_some_and(|s| s.eq_ignore_ascii_case(system_id))
            })
        })
        .and_then(|g| xml::find(g, "ManifestInformation"))
        .and_then(|m| m.attribute("path"))
        .map(str::to_owned))
}

fn updates(scan: &Scan, catalog: &str) -> Result<Vec<Update>, String> {
    let doc = xml::parse(catalog)?;
    let mut updates = Vec::new();
    for component in xml::all(doc.root(), "SoftwareComponent") {
        let kind = xml::child(component, "ComponentType").and_then(|n| n.attribute("value"));
        let Some(offered) = component.attribute("vendorVersion") else {
            continue;
        };
        match kind {
            Some("BIOS") => {
                let Some(installed) = scan.firmware.version.as_deref() else {
                    continue;
                };
                if version::compare(offered, installed) == std::cmp::Ordering::Greater {
                    updates.push(update(component, Kind::Firmware, None, installed));
                }
            }
            Some("DRVR") => {
                for id in pci_hardware_ids(component) {
                    let Some(device) = super::device_with_hardware_id(scan, &id) else {
                        continue;
                    };
                    let Some(installed) = device.driver.as_ref().and_then(|d| d.version.as_deref())
                    else {
                        continue;
                    };
                    if version::newer_than(offered, installed).is_some() {
                        updates.push(update(
                            component,
                            Kind::Driver,
                            Some(&device.instance_id),
                            installed,
                        ));
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(updates)
}

/// Windows hardware ids (`PCI\VEN_8086&DEV_0416&SUBSYS_06041028`) of the
/// devices a component supports.
fn pci_hardware_ids(component: Node) -> Vec<String> {
    xml::all(component, "PCIInfo")
        .filter_map(|p| {
            let (vendor, device) = (p.attribute("vendorID")?, p.attribute("deviceID")?);
            Some(
                match (p.attribute("subVendorID"), p.attribute("subDeviceID")) {
                    (Some(sv), Some(sd)) => {
                        format!("PCI\\VEN_{vendor}&DEV_{device}&SUBSYS_{sd}{sv}")
                    }
                    _ => format!("PCI\\VEN_{vendor}&DEV_{device}"),
                },
            )
        })
        .collect()
}

fn update(component: Node, kind: Kind, device: Option<&str>, installed: &str) -> Update {
    Update {
        source: NAME,
        kind,
        title: xml::child(component, "Name")
            .and_then(|n| xml::text(n, "Display"))
            .unwrap_or_default(),
        version: component.attribute("vendorVersion").map(str::to_owned),
        date: component
            .attribute("dateTime")
            .and_then(|d| d.get(..10))
            .map(str::to_owned),
        url: component.attribute("path").map(|p| format!("{BASE}/{p}")),
        device: device.map(str::to_owned),
        installed: Some(installed.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Device, Driver, Firmware};

    const INDEX: &str = include_str!("testdata/dell-index.xml");
    const CATALOG: &str = include_str!("testdata/dell-07e8.xml");

    #[test]
    fn finds_model_catalog() {
        assert_eq!(
            model_catalog_path(INDEX, "07e8").unwrap().as_deref(),
            Some("FOLDER14968537M/1/Inspiron_Desktop_07E8.cab")
        );
        assert_eq!(model_catalog_path(INDEX, "FFFF").unwrap(), None);
    }

    #[test]
    fn offers_newer_bios_and_matching_driver() {
        let scan = Scan {
            firmware: Firmware {
                version: Some("1.2.0".into()),
                ..Default::default()
            },
            devices: vec![Device {
                instance_id: "PCI\\VEN_8086&DEV_0416\\3".into(),
                hardware_ids: vec![
                    "PCI\\VEN_8086&DEV_0416&SUBSYS_06041028&REV_06".into(),
                    "PCI\\VEN_8086&DEV_0416&SUBSYS_06041028".into(),
                ],
                driver: Some(Driver {
                    version: Some("20.19.15.4300".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        let updates = updates(&scan, CATALOG).unwrap();
        let summary: Vec<_> = updates
            .iter()
            .map(|u| (u.kind, u.version.as_deref()))
            .collect();
        assert_eq!(
            summary,
            [
                (Kind::Driver, Some("20.19.15.4483")),
                (Kind::Firmware, Some("1.10.0"))
            ]
        );
        assert_eq!(updates[0].date.as_deref(), Some("2016-06-15"));
    }
}
