//! Driver updates from Windows Update through the Windows Update Agent COM API.
//! It matches drivers to this machine's hardware ids on Microsoft's side.

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::UpdateAgent::{IUpdateSession, IWindowsDriverUpdate, UpdateSession};
use windows::core::{BSTR, Interface};

use super::Update;
use crate::detect::Error;

const CRITERIA: &str = "IsInstalled=0 and Type='Driver' and IsHidden=0";

pub fn search() -> Result<Vec<Update>, Error> {
    // SAFETY: balanced with CoUninitialize when `_com` drops.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    let _com = ComGuard;

    // SAFETY: plain COM activation and method calls on the returned interfaces.
    unsafe {
        let session: IUpdateSession = CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER)?;
        session.SetClientApplicationID(&BSTR::from("Ferrodrive"))?;
        let searcher = session.CreateUpdateSearcher()?;
        let result = searcher.Search(&BSTR::from(CRITERIA))?;
        let found = result.Updates()?;

        let mut updates = Vec::new();
        for index in 0..found.Count()? {
            let update = found.get_Item(index)?;
            let mut entry = Update {
                source: "Windows Update",
                title: update.Title()?.to_string(),
                ..Default::default()
            };
            if let Ok(driver) = update.cast::<IWindowsDriverUpdate>() {
                entry.hardware_id = driver.DriverHardwareID().ok().map(|s| s.to_string());
                entry.provider = driver.DriverProvider().ok().map(|s| s.to_string());
                entry.class = driver.DriverClass().ok().map(|s| s.to_string());
                entry.driver_date = driver.DriverVerDate().ok().and_then(ole_date);
            }
            updates.push(entry);
        }
        Ok(updates)
    }
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: pairs the successful CoInitializeEx in `search`.
        unsafe { CoUninitialize() };
    }
}

/// OLE automation dates count days since 1899-12-30.
fn ole_date(date: f64) -> Option<String> {
    const DAYS_1899_TO_1970: i64 = 25_569;
    (date > 0.0).then(|| {
        let (y, m, d) = crate::detect::civil_from_days(date.floor() as i64 - DAYS_1899_TO_1970);
        format!("{y:04}-{m:02}-{d:02}")
    })
}
