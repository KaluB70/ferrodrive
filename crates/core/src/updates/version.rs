//! Loose version comparison for the many formats vendors use
//! (`10.1.505.2015`, `9BCN25WW`, `20.19.15.4474_WHQL`).

use std::cmp::Ordering;

/// Compares versions piece by piece, numbers numerically and letters
/// case-insensitively; separators are ignored.
pub fn compare(a: &str, b: &str) -> Ordering {
    let (a, b) = (tokens(a), tokens(b));
    for (x, y) in a.iter().zip(&b) {
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase()),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

/// Dotted version numbers (`1.2`, `10.1.505.2015`) found anywhere in `text`.
pub fn dotted(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|s| s.trim_matches('.'))
        .filter(|s| s.contains('.') && !s.contains(".."))
        .collect()
}

/// The first version in `offered` with the same major number as `installed`
/// that is newer than it.
pub fn newer_than<'a>(offered: &'a str, installed: &str) -> Option<&'a str> {
    let major = |v: &str| v.split('.').next().map(str::to_owned);
    dotted(offered)
        .into_iter()
        .find(|v| major(v) == major(installed) && compare(v, installed) == Ordering::Greater)
}

fn tokens(version: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = None;
    let mut digits = false;
    for (i, c) in version.char_indices() {
        let is_digit = c.is_ascii_digit();
        if !c.is_ascii_alphanumeric() {
            if let Some(s) = start.take() {
                out.push(&version[s..i]);
            }
            continue;
        }
        match start {
            Some(s) if is_digit != digits => {
                out.push(&version[s..i]);
                start = Some(i);
            }
            None => start = Some(i),
            _ => {}
        }
        digits = is_digit;
    }
    if let Some(s) = start {
        out.push(&version[s..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_mixed_versions() {
        assert_eq!(compare("9BCN91WW", "9BCN25WW"), Ordering::Greater);
        assert_eq!(compare("10.1.1.38", "10.1.1.7"), Ordering::Greater);
        assert_eq!(compare("8.66.27.55", "8.66.27.55"), Ordering::Equal);
        assert_eq!(compare("1.2", "1.2.1"), Ordering::Less);
    }

    #[test]
    fn finds_dotted_versions() {
        assert_eq!(
            dotted("ELAN_11.4.94.1_Synaptics_19.0.16.0"),
            ["11.4.94.1", "19.0.16.0"]
        );
        assert_eq!(dotted("20.19.15.4474_WHQL"), ["20.19.15.4474"]);
        assert!(dotted("v1").is_empty());
    }

    #[test]
    fn newer_requires_same_major() {
        assert_eq!(
            newer_than("ELAN_11.4.94.1_Synaptics_19.0.18.0", "19.0.17.96"),
            Some("19.0.18.0")
        );
        assert_eq!(newer_than("20.19.15.4474_WHQL", "20.19.15.4642"), None);
        assert_eq!(newer_than("10.18.13.6871_WHQL", "21.21.13.7667"), None);
    }
}
