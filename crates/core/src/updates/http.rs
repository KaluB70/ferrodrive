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
