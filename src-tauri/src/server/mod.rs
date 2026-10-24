//! Local HTTP server: delivers shared files to the phone and receives
//! files and texts from it.
//!
//! - [`routes`]: landing page, status, text pickup, preview
//! - [`download`]: file and ZIP downloads
//! - [`receive`]: uploads and texts from the phone
//! - [`conn`]: accepting connections with limits and timeouts

mod conn;
mod download;
mod page;
pub mod receive;
mod routes;
mod stream;
mod zip;

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::net::is_private_client;
use crate::share::{EventSink, ShareManager, TextSink};

/// Dynamic/private port range (CLAUDE.md §5.1).
pub const PORT_RANGE: std::ops::RangeInclusive<u16> = 49152..=65535;

/// Read and stream in 64 KiB chunks (CLAUDE.md §5.1).
const CHUNK: usize = 64 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub manager: Arc<ShareManager>,
    pub sink: EventSink,
    /// Texts from the phone go into the clipboard through this.
    pub text_sink: TextSink,
    /// Unfinished, resumable uploads from the phone.
    pub uploads: Arc<receive::Uploads>,
}

pub struct ServerHandle {
    pub port: u16,
    shutdown: Option<oneshot::Sender<()>>,
}

impl ServerHandle {
    /// Stops accepting connections; running transfers get a short grace
    /// period, then their connections are closed.
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn router(state: AppState) -> Router {
    routes::router(state)
        .layer(middleware::from_fn(access_guard))
        .layer(middleware::from_fn(security_headers))
}

/// Only clients from private address ranges are served (CLAUDE.md §5.2),
/// and only under an IP address or `localhost`: a web page on some domain
/// that a DNS rebinding attack points at this computer is turned away.
async fn access_guard(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if is_private_client(addr.ip()) && host_is_address(req.headers()) {
        next.run(req).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

/// True if the `Host` header names an IP address (v4 or bracketed v6) or
/// `localhost`, with an optional port.
fn host_is_address(headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
    else {
        return false;
    };
    let (name, port) = match host.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((v6, after)) => (v6, after.strip_prefix(':')),
            None => return false,
        },
        None => match host.split_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (host, None),
        },
    };
    let port_ok = port.is_none_or(|p| p.parse::<u16>().is_ok());
    let name_ok = name.eq_ignore_ascii_case("localhost")
        || name.parse::<Ipv4Addr>().is_ok()
        || (host.starts_with('[') && name.parse::<Ipv6Addr>().is_ok());
    port_ok && name_ok
}

/// Headers every answer gets: nothing is cached (tokens are secrets and
/// shares are short-lived), no MIME sniffing, no referrer leaking the token.
async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    res
}

/// JSON answer for the phone pages.
fn json(status: StatusCode, body: serde_json::Value) -> Response {
    let mut res = (status, body.to_string()).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    res
}

/// `{"error": code}` with the given status.
fn json_error(status: StatusCode, code: &str) -> Response {
    json(status, serde_json::json!({ "error": code }))
}

fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

/// A port from [`PORT_RANGE`]. Falls back to a clock-derived spread if the
/// system's random source fails, so retries still try different ports.
fn random_port(attempt: u32) -> u16 {
    let mut b = [0u8; 2];
    let seed = if getrandom::fill(&mut b).is_ok() {
        u32::from(u16::from_le_bytes(b))
    } else {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or_default();
        nanos.wrapping_add(attempt.wrapping_mul(7919))
    };
    let span = u32::from(*PORT_RANGE.end() - *PORT_RANGE.start()) + 1;
    // `seed % span` < span <= 16384, so the sum stays within u16.
    *PORT_RANGE.start() + (seed % span) as u16
}

async fn bind(preferred: Option<u16>, host: IpAddr) -> io::Result<TcpListener> {
    if let Some(port) = preferred.filter(|p| PORT_RANGE.contains(p)) {
        if let Ok(l) = TcpListener::bind((host, port)).await {
            return Ok(l);
        }
    }
    let mut last_err = None;
    for attempt in 0..64 {
        match TcpListener::bind((host, random_port(attempt))).await {
            Ok(l) => return Ok(l),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| io::Error::other("no free port")))
}

/// Starts the server on all interfaces. `preferred` is the port from the
/// last run, so the firewall rule / permission stays valid.
pub async fn start(state: AppState, preferred: Option<u16>) -> io::Result<ServerHandle> {
    start_on(state, preferred, Ipv4Addr::UNSPECIFIED).await
}

/// Like [`start`], on one address (tests use localhost).
pub async fn start_on(
    state: AppState,
    preferred: Option<u16>,
    host: Ipv4Addr,
) -> io::Result<ServerHandle> {
    let listener = bind(preferred, IpAddr::V4(host)).await?;
    let port = listener.local_addr()?.port();
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(conn::serve(listener, router(state), rx));
    log::info!("HTTP-Server läuft auf Port {port}");
    Ok(ServerHandle {
        port,
        shutdown: Some(tx),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_str(value).unwrap());
        h
    }

    #[test]
    fn only_address_hosts_are_accepted() {
        for ok in [
            "192.168.178.24:52817",
            "10.0.0.2",
            "127.0.0.1:50000",
            "localhost:50000",
            "[fe80::1]:50000",
            "[::1]",
        ] {
            assert!(host_is_address(&host(ok)), "{ok}");
        }
        for bad in [
            "evil.example:52817",
            "192.168.178.24.nip.io",
            "fe80::1",
            "[fe80::1",
            "192.168.0.1:http",
            "",
        ] {
            assert!(!host_is_address(&host(bad)), "{bad}");
        }
        assert!(!host_is_address(&HeaderMap::new()));
    }

    #[test]
    fn random_ports_stay_in_range() {
        for attempt in 0..100 {
            assert!(PORT_RANGE.contains(&random_port(attempt)));
        }
    }
}
