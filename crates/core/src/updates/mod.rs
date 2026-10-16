//! Update sources: where newer drivers and firmware are looked up.

use serde::Serialize;

use crate::model::{Device, Scan};

mod dell;
mod hp;
mod http;
mod lenovo;
mod thinkpad;
pub mod version;
#[cfg(windows)]
mod windows_update;
mod xml;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Driver,
    Firmware,
}

/// A driver or firmware update offered by a source.
#[derive(Debug, Clone, Serialize)]
pub struct Update {
    pub source: &'static str,
    pub kind: Kind,
    pub title: String,
    pub version: Option<String>,
    /// ISO 8601 date (`YYYY-MM-DD`).
    pub date: Option<String>,
    /// Download page or file, when the source gives one.
    pub url: Option<String>,
    /// Instance id of the device the update is for, when known.
    pub device: Option<String>,
    /// Version installed now (driver version, or BIOS version for firmware).
    pub installed: Option<String>,
}

/// What every source returned; a failing source doesn't hide the others.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub updates: Vec<Update>,
    pub errors: Vec<SourceError>,
}

#[derive(Debug, Serialize)]
pub struct SourceError {
    pub source: &'static str,
    pub message: String,
}

type Source = fn(&Scan) -> Result<Vec<Update>, String>;

/// Asks every source that applies to this machine, all at once.
pub fn check(scan: &Scan) -> Report {
    let mut sources: Vec<(&'static str, Source)> = Vec::new();
    #[cfg(windows)]
    sources.push((windows_update::NAME, windows_update::search));
    if lenovo::applies(scan) {
        sources.push(("Lenovo", lenovo::search));
    }
    if hp::applies(scan) {
        sources.push((hp::NAME, hp::search));
    }
    if dell::applies(scan) {
        sources.push((dell::NAME, dell::search));
    }

    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = sources
            .into_iter()
            .map(|(name, search)| {
                let handle = scope.spawn(move || crate::detect::timed(name, || search(scan)));
                (name, handle)
            })
            .collect();
        handles
            .into_iter()
            .map(|(name, h)| (name, h.join().unwrap_or_else(|_| Err("crashed".into()))))
            .collect()
    });

    let mut report = Report::default();
    for (source, result) in results {
        match result {
            Ok(updates) => report.updates.extend(updates),
            Err(message) => report.errors.push(SourceError { source, message }),
        }
    }
    keep_newest(&mut report.updates);
    report
}

/// Catalogs often list several packages for one device; keep the newest per
/// source, kind and device.
fn keep_newest(updates: &mut Vec<Update>) {
    let mut kept: Vec<Update> = Vec::with_capacity(updates.len());
    for update in updates.drain(..) {
        let same = kept.iter_mut().find(|k| {
            k.source == update.source
                && k.kind == update.kind
                && k.device == update.device
                && update.device.is_some()
                || (k.source == update.source
                    && k.kind == Kind::Firmware
                    && update.kind == Kind::Firmware)
        });
        match same {
            Some(k) => {
                let newer = match (&update.version, &k.version) {
                    (Some(a), Some(b)) => version::compare(a, b).is_gt(),
                    _ => false,
                };
                if newer {
                    *k = update;
                }
            }
            None => kept.push(update),
        }
    }
    *updates = kept;
}

/// The device with this hardware id (case-insensitive).
fn device_with_hardware_id<'a>(scan: &'a Scan, id: &str) -> Option<&'a Device> {
    scan.devices
        .iter()
        .find(|d| d.hardware_ids.iter().any(|h| h.eq_ignore_ascii_case(id)))
}
