//! Windows detection: the configuration manager (CfgMgr32) for devices and drivers, SMBIOS firmware table
//! for BIOS and system identity. Plain Win32 calls, no WMI.

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GETIDLIST_FILTER_PRESENT, CM_Get_DevNode_PropertyW, CM_Get_Device_ID_List_SizeW,
    CM_Get_Device_ID_ListW, CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW, CR_BUFFER_SMALL,
    CR_SUCCESS,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_Class, DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_DriverDate,
    DEVPKEY_Device_DriverInfPath, DEVPKEY_Device_DriverProvider, DEVPKEY_Device_DriverVersion,
    DEVPKEY_Device_FriendlyName, DEVPKEY_Device_HardwareIds, DEVPKEY_Device_Manufacturer,
    DEVPROPTYPE,
};
use windows::Win32::Foundation::DEVPROPKEY;
use windows::Win32::System::SystemInformation::{GetSystemFirmwareTable, RSMB};
use windows::core::PCWSTR;

use super::Error;
use crate::model::{Device, Driver, Scan};
use crate::smbios;

/// Size of the `RawSMBIOSData` header that precedes the SMBIOS structures.
const RAW_SMBIOS_HEADER: usize = 8;

pub fn scan() -> Result<Scan, Error> {
    let (system, firmware) = super::timed("smbios", || {
        smbios_table()
            .map(|t| smbios::parse(&t))
            .unwrap_or_default()
    });
    let devices = super::timed("devices", devices)?;
    Ok(Scan {
        system,
        firmware,
        devices,
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

fn devices() -> Result<Vec<Device>, Error> {
    let ids = device_ids()?;
    let mut buffer = vec![0u8; 4096];
    let mut devices = Vec::with_capacity(ids.len());
    for id in ids {
        let mut devnode = 0u32;
        let id_wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `id_wide` is a NUL-terminated device instance id.
        let located = unsafe {
            CM_Locate_DevNodeW(
                &mut devnode,
                PCWSTR(id_wide.as_ptr()),
                CM_LOCATE_DEVNODE_NORMAL,
            )
        };
        if located == CR_SUCCESS {
            devices.push(device(devnode, id, &mut buffer));
        }
    }
    Ok(devices)
}

/// Instance ids of all present devices, in one call to the configuration manager.
fn device_ids() -> Result<Vec<String>, Error> {
    let flags = CM_GETIDLIST_FILTER_PRESENT;
    loop {
        let mut len = 0u32;
        // SAFETY: size query for the multi-string list.
        let cr = unsafe { CM_Get_Device_ID_List_SizeW(&mut len, PCWSTR::null(), flags) };
        if cr != CR_SUCCESS {
            return Err(Error::ConfigManager(cr.0));
        }
        let mut list = vec![0u16; len as usize];
        // SAFETY: `list` has the size the previous call reported.
        match unsafe { CM_Get_Device_ID_ListW(PCWSTR::null(), &mut list, flags) } {
            CR_SUCCESS => {
                return Ok(list
                    .split(|&c| c == 0)
                    .filter(|s| !s.is_empty())
                    .map(String::from_utf16_lossy)
                    .collect());
            }
            // A device arrived between the two calls; ask for the size again.
            CR_BUFFER_SMALL => continue,
            cr => return Err(Error::ConfigManager(cr.0)),
        }
    }
}

fn device(devnode: u32, instance_id: String, buffer: &mut Vec<u8>) -> Device {
    let mut string = |key| {
        property(devnode, key, buffer)
            .and_then(|raw| wide_strings(raw).next())
            .filter(|s| !s.is_empty())
    };

    let version = string(&DEVPKEY_Device_DriverVersion);
    let provider = string(&DEVPKEY_Device_DriverProvider);
    let package = string(&DEVPKEY_Device_DriverInfPath);
    let name = string(&DEVPKEY_Device_FriendlyName).or_else(|| string(&DEVPKEY_Device_DeviceDesc));
    let class = string(&DEVPKEY_Device_Class);
    let manufacturer = string(&DEVPKEY_Device_Manufacturer);
    let date = property(devnode, &DEVPKEY_Device_DriverDate, buffer).and_then(filetime_to_date);
    let hardware_ids = property(devnode, &DEVPKEY_Device_HardwareIds, buffer)
        .map(|raw| wide_strings(raw).collect())
        .unwrap_or_default();

    Device {
        instance_id,
        name,
        class,
        manufacturer,
        hardware_ids,
        driver: version.is_some().then_some(Driver {
            version,
            date,
            provider,
            package,
        }),
    }
}

/// Reads a device property into `buffer` (growing it when needed) and returns
/// the filled part; `None` when the device doesn't have the property.
fn property<'a>(devnode: u32, key: &DEVPROPKEY, buffer: &'a mut Vec<u8>) -> Option<&'a [u8]> {
    loop {
        let mut kind = DEVPROPTYPE::default();
        let mut size = buffer.len() as u32;
        // SAFETY: `size` is the capacity of `buffer`, which the call writes at most.
        let cr = unsafe {
            CM_Get_DevNode_PropertyW(
                devnode,
                key,
                &mut kind,
                Some(buffer.as_mut_ptr()),
                &mut size,
                0,
            )
        };
        match cr {
            CR_SUCCESS => return Some(&buffer[..size as usize]),
            CR_BUFFER_SMALL => buffer.resize(size as usize, 0),
            _ => return None,
        }
    }
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
