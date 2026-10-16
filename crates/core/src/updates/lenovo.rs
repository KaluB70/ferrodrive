//! Lenovo consumer and business models through the JSON API behind
//! pcsupport.lenovo.com. It is not a documented API, so treat its results as
//! best effort and expect it to change.

use serde::Deserialize;

use super::{Kind, Update, http, version};
use crate::model::{Device, Scan};

pub const NAME: &str = "Lenovo support (unofficial)";

const API: &str = "https://pcsupport.lenovo.com/us/en/api/v4";
/// The API answers "deny access" without a Referer from its own site.
const HEADERS: [(&str, &str); 2] = [
    ("Referer", "https://pcsupport.lenovo.com/"),
    ("Accept", "application/json"),
];
const OPERATING_SYSTEMS: [&str; 2] = ["Windows 11 (64-bit)", "Windows 10 (64-bit)"];

pub fn applies(scan: &Scan) -> bool {
    scan.system
        .manufacturer
        .as_deref()
        .is_some_and(|m| m.eq_ignore_ascii_case("LENOVO"))
        && scan.system.product.is_some()
}

pub fn search(scan: &Scan) -> Result<Vec<Update>, String> {
    let product = scan.system.product.as_deref().unwrap_or_default();
    let products: Vec<Product> = json(&format!("{API}/mse/getproducts?productId={product}"))?;
    let Some(product) = products.into_iter().next() else {
        return Err(format!("Lenovo doesn't know product {product}"));
    };
    let drivers: Drivers = json(&format!("{API}/downloads/drivers?productId={}", product.id))?;
    Ok(updates(scan, &drivers.body.download_items))
}

fn json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
    let body = http::get(url, &HEADERS)?;
    serde_json::from_str(&body).map_err(|e| format!("{url}: unexpected response ({e})"))
}

fn updates(scan: &Scan, items: &[Item]) -> Vec<Update> {
    let mut updates = Vec::new();
    for item in items.iter().filter(|i| i.for_this_os()) {
        let Some(file) = item.installer() else {
            continue;
        };
        if item.category.name == "BIOS/UEFI" {
            updates.extend(bios_update(scan, item, file));
        } else {
            updates.extend(driver_updates(scan, item, file));
        }
    }
    updates
}

/// Lenovo names BIOS packages after the BIOS build (`9bcn91ww.exe` for 9BCN91WW),
/// so the file name compares directly with the installed BIOS version.
fn bios_update(scan: &Scan, item: &Item, file: &File) -> Option<Update> {
    let installed = scan.firmware.version.as_deref()?;
    let stem = file.url.rsplit('/').next()?.split('.').next()?;
    let prefix = installed.get(..4)?;
    if !stem.get(..4)?.eq_ignore_ascii_case(prefix)
        || version::compare(stem, installed) != std::cmp::Ordering::Greater
    {
        return None;
    }
    Some(update(
        item,
        file,
        Kind::Firmware,
        None,
        installed,
        Some(stem.to_ascii_uppercase()),
    ))
}

/// Matches a driver package to devices by category and vendor name, and
/// offers it where one of its versions is newer than the installed driver.
fn driver_updates(scan: &Scan, item: &Item, file: &File) -> Vec<Update> {
    let classes = classes_for(&item.category.name);
    let text = format!("{} {}", item.title, file.version).to_ascii_lowercase();
    scan.devices
        .iter()
        .filter(|d| d.class.as_deref().is_some_and(|c| classes.contains(&c)))
        .filter(|d| mentions_vendor(&text, d))
        .filter_map(|d| {
            let installed = d.driver.as_ref()?.version.as_deref()?;
            let newer = version::newer_than(&file.version, installed)?;
            Some(update(
                item,
                file,
                Kind::Driver,
                Some(d),
                installed,
                Some(newer.to_owned()),
            ))
        })
        .collect()
}

fn update(
    item: &Item,
    file: &File,
    kind: Kind,
    device: Option<&Device>,
    installed: &str,
    version: Option<String>,
) -> Update {
    Update {
        source: NAME,
        kind,
        title: item.title.clone(),
        version,
        date: file.date.as_ref().and_then(|d| unix_millis_to_date(d.unix)),
        url: Some(file.url.clone()),
        device: device.map(|d| d.instance_id.clone()),
        installed: Some(installed.to_owned()),
    }
}

/// Windows device classes a Lenovo download category can update.
fn classes_for(category: &str) -> &'static [&'static str] {
    match category {
        "Audio" => &["MEDIA"],
        "Display and Video Graphics" => &["Display"],
        "Networking: Wireless LAN" | "Networking: LAN (Ethernet)" => &["Net"],
        "Bluetooth and Modem" => &["Bluetooth"],
        "Camera and Card Reader" => &["Camera", "Image", "USB"],
        "Mouse, Touchpad, Keyboard and Pen" => &["Mouse", "HIDClass", "Keyboard"],
        "Storage" => &["HDC", "SCSIAdapter"],
        c if c.starts_with("Motherboard Devices") => &["System"],
        _ => &[],
    }
}

/// True when the package text names the device's vendor, or names no
/// known vendor at all (e.g. "Audio Driver for Windows 10").
fn mentions_vendor(text: &str, device: &Device) -> bool {
    const VENDORS: [&str; 12] = [
        "intel",
        "nvidia",
        "amd",
        "realtek",
        "conexant",
        "synaptics",
        "elan",
        "broadcom",
        "atheros",
        "qualcomm",
        "alcor",
        "bison",
    ];
    let named: Vec<&str> = VENDORS
        .iter()
        .copied()
        .filter(|v| text.contains(v))
        .collect();
    let provider = device.driver.as_ref().and_then(|d| d.provider.as_deref());
    if provider.is_some_and(|p| p.eq_ignore_ascii_case("Microsoft")) {
        return false;
    }
    if named.is_empty() {
        return true;
    }
    let identity = [
        device.name.as_deref(),
        device.manufacturer.as_deref(),
        provider,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();
    named.iter().any(|v| identity.contains(v))
}

fn unix_millis_to_date(millis: i64) -> Option<String> {
    let (y, m, d) = crate::detect::civil_from_days(millis.div_euclid(86_400_000));
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

#[derive(Deserialize)]
struct Product {
    #[serde(rename = "Id")]
    id: String,
}

#[derive(Deserialize)]
struct Drivers {
    body: DriversBody,
}

#[derive(Deserialize)]
struct DriversBody {
    #[serde(rename = "DownloadItems", default)]
    download_items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    #[serde(rename = "Title")]
    title: String,
    #[serde(rename = "Category")]
    category: Category,
    #[serde(rename = "OperatingSystemKeys", default)]
    operating_systems: Vec<String>,
    #[serde(rename = "Files", default)]
    files: Vec<File>,
}

impl Item {
    fn for_this_os(&self) -> bool {
        self.operating_systems
            .iter()
            .any(|os| OPERATING_SYSTEMS.contains(&os.as_str()))
    }

    /// The package itself, not its README.
    fn installer(&self) -> Option<&File> {
        self.files
            .iter()
            .find(|f| matches!(f.kind.as_str(), "EXE" | "ZIP" | "zip" | "MSI"))
    }
}

#[derive(Deserialize)]
struct Category {
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Deserialize)]
struct File {
    #[serde(rename = "TypeString")]
    kind: String,
    #[serde(rename = "Version", default)]
    version: String,
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "Date")]
    date: Option<UnixDate>,
}

#[derive(Deserialize)]
struct UnixDate {
    #[serde(rename = "Unix")]
    unix: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Driver, Firmware, System};

    const SAMPLE: &str = include_str!("testdata/lenovo-z50-70.json");

    fn scan() -> Scan {
        let device = |class: &str, name: &str, provider: &str, version: &str| Device {
            instance_id: name.into(),
            name: Some(name.into()),
            class: Some(class.into()),
            driver: Some(Driver {
                version: Some(version.into()),
                provider: Some(provider.into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        Scan {
            system: System {
                manufacturer: Some("LENOVO".into()),
                product: Some("20354".into()),
                ..Default::default()
            },
            firmware: Firmware {
                version: Some("9BCN25WW".into()),
                ..Default::default()
            },
            devices: vec![
                device(
                    "Display",
                    "Intel(R) HD Graphics Family",
                    "Intel Corporation",
                    "20.19.15.4642",
                ),
                device(
                    "Mouse",
                    "Synaptics Pointing Device",
                    "Synaptics",
                    "19.0.15.1",
                ),
                device("MEDIA", "Conexant SmartAudio HD", "Conexant", "8.66.27.55"),
                device(
                    "Net",
                    "Realtek PCIe GBE Family Controller",
                    "Realtek",
                    "10.1.505.2015",
                ),
            ],
        }
    }

    #[test]
    fn offers_newer_bios_and_drivers_only() {
        let drivers: Drivers = serde_json::from_str(SAMPLE).unwrap();
        let updates = updates(&scan(), &drivers.body.download_items);

        let summary: Vec<_> = updates
            .iter()
            .map(|u| (u.kind, u.device.as_deref(), u.version.as_deref()))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    Kind::Driver,
                    Some("Synaptics Pointing Device"),
                    Some("19.0.16.0")
                ),
                (Kind::Firmware, None, Some("9BCN91WW")),
            ]
        );
        assert_eq!(updates[1].date.as_deref(), Some("2015-09-24"));
    }
}
