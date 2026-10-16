use std::time::Duration;

const USER_AGENT: &str = concat!("Ferrodrive/", env!("CARGO_PKG_VERSION"));

/// GETs `url` as text, sending `headers` and failing on non-2xx responses.
pub fn get(url: &str, headers: &[(&str, &str)]) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .user_agent(USER_AGENT)
        .build()
        .into();
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request
        .call()
        .and_then(|mut response| {
            response
                .body_mut()
                .with_config()
                .limit(64 << 20)
                .read_to_string()
        })
        .map_err(|e| format!("{url}: {e}"))
}

/// GETs `url` as bytes; `Ok(None)` on 404 so callers can treat a missing
/// catalog as "this source doesn't cover the machine".
pub fn get_bytes(url: &str) -> Result<Option<Vec<u8>>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent(USER_AGENT)
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    match response.status().as_u16() {
        404 => Ok(None),
        200..=299 => response
            .body_mut()
            .with_config()
            .limit(256 << 20)
            .read_to_vec()
            .map(Some)
            .map_err(|e| format!("{url}: {e}")),
        status => Err(format!("{url}: HTTP {status}")),
    }
}

/// Decodes catalog text: UTF-8, or UTF-16 when it starts with a byte order mark.
pub fn decode(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn utf16(bytes: &[u8], read: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| read([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

/// The text of the first file in a CAB archive (vendors ship one XML per cab).
pub fn first_file_in_cab(bytes: Vec<u8>) -> Result<String, String> {
    let mut cabinet =
        cab::Cabinet::new(std::io::Cursor::new(bytes)).map_err(|e| format!("bad cab: {e}"))?;
    let name = cabinet
        .folder_entries()
        .flat_map(|f| f.file_entries())
        .map(|f| f.name().to_owned())
        .next()
        .ok_or("empty cab")?;
    let mut out = Vec::new();
    std::io::Read::read_to_end(
        &mut cabinet
            .read_file(&name)
            .map_err(|e| format!("cab {name}: {e}"))?,
        &mut out,
    )
    .map_err(|e| format!("cab {name}: {e}"))?;
    Ok(decode(&out))
}

/// GETs a CAB and returns its first file's text; `Ok(None)` on 404.
pub fn get_cab(url: &str) -> Result<Option<String>, String> {
    get_bytes(url)?.map(first_file_in_cab).transpose()
}
