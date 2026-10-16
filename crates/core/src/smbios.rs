//! Minimal SMBIOS table parser for the BIOS (type 0), system (type 1) and
//! baseboard (type 2) structures.

use crate::model::{Firmware, System};

const TYPE_BIOS: u8 = 0;
const TYPE_SYSTEM: u8 = 1;
const TYPE_BASEBOARD: u8 = 2;
const TYPE_END: u8 = 127;

/// Parses raw SMBIOS structures (without any OS-specific header).
pub fn parse(table: &[u8]) -> (System, Firmware) {
    let mut system = System::default();
    let mut firmware = Firmware::default();

    for structure in Structures::new(table) {
        match structure.kind {
            TYPE_BIOS => {
                firmware.vendor = structure.string_at(0x04);
                firmware.version = structure.string_at(0x05);
                firmware.release_date = structure.string_at(0x08).map(|d| normalize_date(&d));
            }
            TYPE_SYSTEM => {
                system.manufacturer = structure.string_at(0x04);
                system.product = structure.string_at(0x05);
                system.version = structure.string_at(0x06);
                // SMBIOS 2.4+ only; older tables are shorter and yield None.
                system.sku = structure.string_at(0x19);
                system.family = structure.string_at(0x1a);
            }
            TYPE_BASEBOARD => {
                system.board_manufacturer = structure.string_at(0x04);
                system.board_product = structure.string_at(0x05);
            }
            _ => {}
        }
    }

    (system, firmware)
}

/// SMBIOS dates are `MM/DD/YYYY` (or `MM/DD/YY` before 2.3); returns ISO 8601
/// when the input matches, else the input unchanged.
fn normalize_date(date: &str) -> String {
    let parts: Vec<&str> = date.split('/').collect();
    match parts.as_slice() {
        [m, d, y] if m.len() == 2 && d.len() == 2 && y.len() == 4 => format!("{y}-{m}-{d}"),
        [m, d, y] if m.len() == 2 && d.len() == 2 && y.len() == 2 => format!("19{y}-{m}-{d}"),
        _ => date.to_owned(),
    }
}

struct Structure<'a> {
    kind: u8,
    formatted: &'a [u8],
    strings: Vec<&'a [u8]>,
}

impl Structure<'_> {
    /// Reads the string whose 1-based index is stored at `offset` of the formatted area.
    fn string_at(&self, offset: usize) -> Option<String> {
        let index = *self.formatted.get(offset)? as usize;
        let raw = self.strings.get(index.checked_sub(1)?)?;
        let value = String::from_utf8_lossy(raw).trim().to_owned();
        (!value.is_empty() && !is_placeholder(&value)).then_some(value)
    }
}

fn is_placeholder(value: &str) -> bool {
    const PLACEHOLDERS: [&str; 4] = [
        "To Be Filled By O.E.M.",
        "Default string",
        "System Product Name",
        "Not Applicable",
    ];
    PLACEHOLDERS.iter().any(|p| value.eq_ignore_ascii_case(p))
}

struct Structures<'a> {
    rest: &'a [u8],
}

impl<'a> Structures<'a> {
    fn new(table: &'a [u8]) -> Self {
        Self { rest: table }
    }
}

impl<'a> Iterator for Structures<'a> {
    type Item = Structure<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let kind = *self.rest.first()?;
        let length = *self.rest.get(1)? as usize;
        if kind == TYPE_END || length < 4 || self.rest.len() < length {
            return None;
        }
        let formatted = &self.rest[..length];

        // The string set ends with a double NUL; a structure without strings has just "\0\0".
        let tail = &self.rest[length..];
        let end = tail.windows(2).position(|w| w == [0, 0])?;
        let strings = tail[..end]
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .collect();

        self.rest = &tail[end + 2..];
        Some(Structure {
            kind,
            formatted,
            strings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure(kind: u8, formatted_len: u8, fields: &[(usize, u8)], strings: &[&str]) -> Vec<u8> {
        let mut out = vec![0u8; formatted_len as usize];
        out[0] = kind;
        out[1] = formatted_len;
        for &(offset, value) in fields {
            out[offset] = value;
        }
        for s in strings {
            out.extend_from_slice(s.as_bytes());
            out.push(0);
        }
        if strings.is_empty() {
            out.push(0);
        }
        out.push(0);
        out
    }

    #[test]
    fn parses_bios_system_and_board() {
        let mut table = structure(
            0,
            0x18,
            &[(4, 1), (5, 2), (8, 3)],
            &["Dell Inc.", "1.21.0", "03/14/2025"],
        );
        table.extend(structure(
            1,
            0x1b,
            &[(4, 1), (5, 2), (6, 0)],
            &["Dell Inc.", "XPS 15 9530"],
        ));
        table.extend(structure(
            2,
            0x0f,
            &[(4, 1), (5, 2)],
            &["Dell Inc.", "To Be Filled By O.E.M."],
        ));
        table.extend(structure(127, 4, &[], &[]));

        let (system, firmware) = parse(&table);

        assert_eq!(firmware.vendor.as_deref(), Some("Dell Inc."));
        assert_eq!(firmware.version.as_deref(), Some("1.21.0"));
        assert_eq!(firmware.release_date.as_deref(), Some("2025-03-14"));
        assert_eq!(system.product.as_deref(), Some("XPS 15 9530"));
        assert_eq!(system.version, None);
        assert_eq!(system.board_product, None);
    }

    #[test]
    fn stops_on_truncated_table() {
        let table = [0u8, 0x18, 1, 2];
        assert_eq!(parse(&table), (System::default(), Firmware::default()));
    }
}
