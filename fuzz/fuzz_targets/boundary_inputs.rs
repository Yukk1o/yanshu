#![forbid(unsafe_code)]
#![no_main]

use std::{str, sync::OnceLock};

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderName, HeaderValue, Method, Uri, header::CONTENT_TYPE},
};
use libfuzzer_sys::fuzz_target;
use yanshu_bundle::parse_bundle_manifest_bytes;
use yanshu_http::{HttpConfig, normalize_http_request};
use yanshu_package::{
    parse_package_lock_bytes, parse_package_manifest_bytes, parse_package_source_bytes,
};

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap_or_else(|_| panic!("static fuzz runtime must remain constructible"))
    })
}

fn fuzz_http(data: &[u8]) {
    let flags = data.first().copied().unwrap_or_default();
    let requested_headers = data.get(1).copied().unwrap_or_default() as usize;
    let uri_length = data
        .get(2..4)
        .and_then(|bytes| <[u8; 2]>::try_from(bytes).ok())
        .map_or(0, u16::from_le_bytes) as usize;
    let uri_end = 4_usize.saturating_add(uri_length).min(data.len());
    let uri_bytes = data.get(4..uri_end).unwrap_or_default();
    let body = data.get(uri_end..).unwrap_or_default();
    let uri = str::from_utf8(uri_bytes)
        .ok()
        .and_then(|value| value.parse::<Uri>().ok())
        .unwrap_or_else(|| Uri::from_static("/fuzz"));
    let method = match flags % 5 {
        0 => Method::GET,
        1 => Method::POST,
        2 => Method::PUT,
        3 => Method::PATCH,
        _ => Method::DELETE,
    };
    let Ok(mut request) = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from(body.to_vec()))
    else {
        return;
    };
    if flags & 0b0001_0000 != 0 {
        request
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    }
    for index in 0..requested_headers.min(128) {
        let name = format!("x-fuzz-{index}");
        let Ok(name) = HeaderName::try_from(name) else {
            continue;
        };
        let value = HeaderValue::from_str(&format!("{}", flags.wrapping_add(index as u8)))
            .unwrap_or_else(|_| HeaderValue::from_static("0"));
        request.headers_mut().insert(name, value);
    }
    if flags & 0b0010_0000 != 0
        && let Some(value) = body.get(..body.len().min(70 * 1024))
        && let Ok(value) = HeaderValue::from_bytes(value)
    {
        request
            .headers_mut()
            .insert(HeaderName::from_static("x-fuzz-large"), value);
    }
    let _ = runtime().block_on(normalize_http_request(request, &HttpConfig::default()));
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, payload)) = data.split_first() else {
        return;
    };
    match selector % 5 {
        0 => {
            let _ = parse_bundle_manifest_bytes(payload);
        }
        1 => {
            let _ = parse_package_source_bytes(payload);
        }
        2 => {
            let _ = parse_package_manifest_bytes(payload);
        }
        3 => {
            let _ = parse_package_lock_bytes(payload);
        }
        _ => fuzz_http(payload),
    }
});
