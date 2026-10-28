// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Chiamate firmate del dispositivo verso `/api/clanto/v1/device/*`
//! (`clantodesk-server/docs/api-future.md`). Corpo e query entrano nella firma
//! `ClantoDevice`; gli errori `clock` e `revoked` passano da
//! `enrollment::observe_response`.

use super::enrollment;
use hbb_common::{bail, tls::TlsType, tokio, ResultType};
use serde_json::Value;
use std::time::Duration;

pub const API_PREFIX: &str = "/api/clanto/v1";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// URL completo di un endpoint del dispositivo; `None` se non iscritto.
pub fn url(path: &str) -> Option<String> {
    enrollment::device_api().map(|api| format!("{api}{API_PREFIX}{path}"))
}

async fn client_for(url: &str) -> ResultType<reqwest::Client> {
    if url.starts_with("https://") {
        crate::hbbs_http::create_http_client_async_with_url_strict(url).await
    } else {
        // Solo loopback (enrollment::is_secure_url), per i test.
        Ok(crate::hbbs_http::create_http_client_async(TlsType::Plain, false))
    }
}

/// Richiesta firmata. Restituisce stato e corpo; non fallisce sui codici HTTP.
pub async fn request(
    method: &str,
    url: &str,
    body: Vec<u8>,
    content_type: &str,
    timeout: Duration,
) -> ResultType<(u16, Vec<u8>)> {
    let Some(auth) = enrollment::auth_value(method, url, &body) else {
        bail!("device not enrolled");
    };
    let client = client_for(url).await?;
    let builder = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        _ => bail!("unsupported method {method}"),
    };
    let mut builder = builder
        .header("Authorization", auth)
        .timeout(timeout);
    if !body.is_empty() {
        builder = builder.header("Content-Type", content_type).body(body);
    }
    let response = builder.send().await?;
    let status = response.status().as_u16();
    let bytes = response.bytes().await?.to_vec();
    enrollment::observe_response(status, &String::from_utf8_lossy(&bytes));
    Ok((status, bytes))
}

fn parse(status: u16, bytes: &[u8]) -> ResultType<Value> {
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice::<Value>(bytes).unwrap_or(Value::Null)
    };
    if !(200..300).contains(&status) {
        let code = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("http_error")
            .to_owned();
        bail!("HTTP {status}: {code}");
    }
    Ok(value)
}

pub async fn get_json(url: &str, timeout: Duration) -> ResultType<Value> {
    let (status, bytes) = request("GET", url, Vec::new(), "", timeout).await?;
    parse(status, &bytes)
}

pub async fn post_json(url: &str, body: &Value) -> ResultType<Value> {
    let (status, bytes) = request(
        "POST",
        url,
        body.to_string().into_bytes(),
        "application/json",
        DEFAULT_TIMEOUT,
    )
    .await?;
    parse(status, &bytes)
}

pub async fn post_bytes(url: &str, body: Vec<u8>, timeout: Duration) -> ResultType<Value> {
    let (status, bytes) =
        request("POST", url, body, "application/octet-stream", timeout).await?;
    parse(status, &bytes)
}

fn runtime() -> ResultType<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

/// Versione bloccante, da thread senza runtime.
pub fn post_json_sync(url: &str, body: &Value) -> ResultType<Value> {
    runtime()?.block_on(post_json(url, body))
}

pub fn get_json_sync(url: &str, timeout: Duration) -> ResultType<Value> {
    runtime()?.block_on(get_json(url, timeout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_is_reported() {
        let err = parse(403, br#"{"error":"revoked"}"#).unwrap_err();
        assert_eq!(err.to_string(), "HTTP 403: revoked");
        assert!(parse(200, b"").unwrap().is_null());
        assert_eq!(parse(200, br#"{"a":1}"#).unwrap()["a"], 1);
    }
}
