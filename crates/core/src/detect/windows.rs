//! Windows detection: SetupAPI for devices and drivers, SMBIOS firmware table
//! for BIOS and system identity. Plain Win32 calls, no WMI.

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList,
    SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW,
    SetupDiGetDevicePropertyW,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_Class, DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_DriverDate,
    DEVPKEY_Device_DriverInfPath, DEVPKEY_Device_DriverProvider, DEVPKEY_Device_DriverVersion,
    DEVPKEY_Device_FriendlyName, DEVPKEY_Device_HardwareIds, DEVPKEY_Device_Manufacturer,
    DEVPROPTYPE,
};
use windows::Win32::Foundation::{DEVPROPKEY, ERROR_NO_MORE_ITEMS};
use windows::Win32::System::SystemInformation::{GetSystemFirmwareTable, RSMB};
use windows::core::PCWSTR;

use super::Error;
use crate::model::{Device, Driver, Scan};
use crate::smbios;

/// Size of the `RawSMBIOSData` header that precedes the SMBIOS structures.
const RAW_SMBIOS_HEADER: usize = 8;

pub fn scan() -> Result<Scan, Error> {
    let (system, firmware) = smbios_table()
        .map(|t| smbios::parse(&t))
        .unwrap_or_default();
    Ok(Scan {
        system,
        firmware,
        devices: devices()?,
    })
}

fn smbios_table() -> Option<Vec<u8>> {
    // SAFETY: first call only queries the size; the second writes into a buffer of that size.
    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, None) } as usize;
    if size <= RAW_SMBIOS_HEADER {
        return None;
    }
    let mut buffer = vec![0u8; size];
    let written = unsafe { GetSystemFirmwareTable(RSMB, 0, Some(&mut buffer)) } as usize;
    if written != size {
        return None;
    }
    let length = u32::from_le_bytes(buffer[4..8].try_into().ok()?) as usize;
    buffer.drain(..RAW_SMBIOS_HEADER);
    buffer.truncate(length);
    Some(buffer)
}

/// Owns a SetupAPI device information set and frees it on drop.
struct DeviceInfoSet(HDEVINFO);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        // SAFETY: the handle came from SetupDiGetClassDevsW and is freed exactly once.
        let _ = unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

fn devices() -> Result<Vec<Device>, Error> {
    // SAFETY: all-classes query of present devices; the handle is owned by `DeviceInfoSet`.
    let set = DeviceInfoSet(unsafe {
        SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_ALLCLASSES | DIGCF_PRESENT)?
    });

    let mut devices = Vec::new();
    for index in 0.. {
        let mut info = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is a correctly sized SP_DEVINFO_DATA owned by this frame.
        match unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut info) } {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_NO_MORE_ITEMS.to_hresult() => break,
            Err(e) => return Err(e.into()),
        }
        devices.push(device(&set, &info));
    }
    Ok(devices)
}

fn device(set: &DeviceInfoSet, info: &SP_DEVINFO_DATA) -> Device {
    let string =
        |key| property(set, info, key).map(|raw| wide_strings(&raw).next().unwrap_or_default());
    let string = |key| string(key).filter(|s: &String| !s.is_empty());

    let version = string(&DEVPKEY_Device_DriverVersion);
    let driver = version.is_some().then(|| Driver {
        version,
        date: property(set, info, &DEVPKEY_Device_DriverDate)
            .and_then(|raw| filetime_to_date(&raw)),
        provider: string(&DEVPKEY_Device_DriverProvider),
        package: string(&DEVPKEY_Device_DriverInfPath),
    });

    Device {
        instance_id: instance_id(set, info).unwrap_or_default(),
        name: string(&DEVPKEY_Device_FriendlyName).or_else(|| string(&DEVPKEY_Device_DeviceDesc)),
        class: string(&DEVPKEY_Device_Class),
        manufacturer: string(&DEVPKEY_Device_Manufacturer),
        hardware_ids: property(set, info, &DEVPKEY_Device_HardwareIds)
            .map(|raw| wide_strings(&raw).collect())
            .unwrap_or_default(),
        driver,
    }
}

/// Reads a device property as raw bytes; `None` when the device doesn't have it.
fn property(set: &DeviceInfoSet, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<Vec<u8>> {
    let mut kind = DEVPROPTYPE::default();
    let mut size = 0u32;
    // SAFETY: size query (fails with ERROR_INSUFFICIENT_BUFFER), then a read into a buffer of that size.
    let _ =
        unsafe { SetupDiGetDevicePropertyW(set.0, info, key, &mut kind, None, Some(&mut size), 0) };
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    unsafe { SetupDiGetDevicePropertyW(set.0, info, key, &mut kind, Some(&mut buffer), None, 0) }
        .ok()?;
    Some(buffer)
}

fn instance_id(set: &DeviceInfoSet, info: &SP_DEVINFO_DATA) -> Option<String> {
    let mut buffer = [0u16; 512];
    // SAFETY: the buffer is larger than MAX_DEVICE_ID_LEN (200).
    unsafe { SetupDiGetDeviceInstanceIdW(set.0, info, Some(&mut buffer), None) }.ok()?;
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

/// Splits a UTF-16LE (multi-)string property into its non-empty parts.
fn wide_strings(raw: &[u8]) -> impl Iterator<Item = String> + '_ {
    let wide: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    wide.split(|&c| c == 0)
        .filter(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect::<Vec<_>>()
        .into_iter()
}

/// Converts a FILETIME (100 ns ticks since 1601-01-01) to `YYYY-MM-DD`.
fn filetime_to_date(raw: &[u8]) -> Option<String> {
    const TICKS_PER_DAY: u64 = 864_000_000_000;
    const DAYS_1601_TO_1970: i64 = 134_774;
    let ticks = u64::from_le_bytes(raw.get(..8)?.try_into().ok()?);
    let days = (ticks / TICKS_PER_DAY) as i64 - DAYS_1601_TO_1970;
    let (y, m, d) = crate::detect::civil_from_days(days);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}
