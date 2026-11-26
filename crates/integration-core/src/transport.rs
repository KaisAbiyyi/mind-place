//! Strict, single-endpoint loopback Streamable HTTP MCP adapter.
//! No SSE/session/tunnel, no cookies, no CORS, no raw filesystem endpoints.
//! Never launch this listener before explicit trusted-owner unlock/start.
use crate::mcp::{self, McpReply, BODY_LIMIT, MCP_PORT};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST, ORIGIN};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use mind_place_archive::ArchiveStore;
#[cfg(feature = "vault-mcp")]
use mind_place_index::vault_archive::VaultCaptureStore;
use mind_place_index::IndexStore;
use mind_place_security::SecurityGate;
use mind_place_state::StateStore;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{Notify, Semaphore};
use zeroize::Zeroizing;

const MAX_WIRE_BYTES: usize = 1024 * 1024;
const IO_DEADLINE: Duration = Duration::from_secs(35);
const MAX_CONNECTIONS: usize = 8;
// Outlive HTTP deadlines: timed-out spawn_blocking queries must retain a slot
// until they actually finish, rather than spawning unlimited work on retries.
const MAX_DISPATCH_TASKS: usize = 4;

type WireResponse = Response<Full<Bytes>>;

fn wire(status: u16, value: Option<serde_json::Value>) -> WireResponse {
    let bytes = value
        .and_then(|body| serde_json::to_vec(&body).ok())
        .unwrap_or_default();
    let (code, bytes) = if bytes.len() > MAX_WIRE_BYTES {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
                .to_vec(),
        )
    } else {
        (
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            bytes,
        )
    };
    Response::builder()
        .status(code)
        .header(CONTENT_TYPE, "application/json")
        .header("cache-control", "no-store")
        .header("x-content-type-options", "nosniff")
        .header("connection", "close")
        .body(Full::new(Bytes::from(bytes)))
        .expect("static HTTP headers")
}

fn error(status: u16) -> WireResponse {
    wire(status, None)
}

fn is_http_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'\x60'
                        | b'|'
                        | b'~'
                )
        })
}

// Split a list/parameter sequence without treating separators inside quoted
// parameter values as delimiters. Invalid quoting and line breaks fail closed.
fn split_quoted(value: &str, separator: u8) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in value.bytes().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
                continue;
            }
            match byte {
                b'\\' => escaped = true,
                b'"' => quoted = false,
                b'\r' | b'\n' => return None,
                _ => {}
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'\r' | b'\n' => return None,
                byte if byte == separator => {
                    let part = value[start..index].trim();
                    if part.is_empty() {
                        return None;
                    }
                    parts.push(part);
                    start = index + 1;
                }
                _ => {}
            }
        }
    }
    if quoted || escaped {
        return None;
    }
    let last = value[start..].trim();
    if last.is_empty() {
        return None;
    }
    parts.push(last);
    Some(parts)
}

fn valid_quoted_string(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return false;
    }
    let mut at = 1usize;
    while at + 1 < bytes.len() {
        let byte = bytes[at];
        if byte == b'\\' {
            at += 1;
            if at + 1 >= bytes.len() {
                return false;
            }
            let escaped = bytes[at];
            if !(escaped == b'\t' || (0x20..=0x7e).contains(&escaped) || escaped >= 0x80) {
                return false;
            }
        } else if byte == b'"'
            || !(byte == b'\t'
                || byte == b' '
                || byte == 0x21
                || (0x23..=0x5b).contains(&byte)
                || (0x5d..=0x7e).contains(&byte)
                || byte >= 0x80)
        {
            return false;
        }
        at += 1;
    }
    true
}

fn valid_parameter_value(value: &str) -> bool {
    if value.starts_with('"') {
        valid_quoted_string(value)
    } else {
        is_http_token(value)
    }
}

// RFC qvalue precision is limited to three decimal places. Return thousandths
// so q=0 is exact and no floating-point rounding can change acceptance.
fn parse_quality(value: &str) -> Option<u16> {
    let (whole, fraction) = match value.split_once('.') {
        Some((whole, fraction)) if !fraction.contains('.') => (whole, fraction),
        Some(_) => return None,
        None => (value, ""),
    };
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    match whole {
        "0" => {
            let digits = if fraction.is_empty() {
                0
            } else {
                fraction.parse::<u16>().ok()?
            };
            Some(digits * 10u16.pow((3 - fraction.len()) as u32))
        }
        "1" if fraction.bytes().all(|byte| byte == b'0') => Some(1000),
        _ => None,
    }
}

fn parse_accept_item(item: &str) -> Option<(&str, u16)> {
    let parts = split_quoted(item, b';')?;
    let media_type = parts.first()?.trim();
    let (type_name, subtype) = media_type.split_once('/')?;
    if subtype.contains('/') || !is_http_token(type_name) || !is_http_token(subtype) {
        return None;
    }

    let mut quality = 1000u16;
    let mut quality_seen = false;
    let mut parameter_names: Vec<String> = Vec::new();
    for parameter in parts.iter().skip(1) {
        if let Some((name, value)) = parameter.split_once('=') {
            let name = name.trim();
            let value = value.trim();
            if !is_http_token(name) || !valid_parameter_value(value) {
                return None;
            }
            if name.eq_ignore_ascii_case("q") {
                if quality_seen {
                    return None;
                }
                quality_seen = true;
                quality = parse_quality(value)?;
            } else {
                if parameter_names
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(name))
                {
                    return None;
                }
                parameter_names.push(name.to_owned());
            }
        } else {
            // A valueless accept-extension is legal only after the weight.
            if !quality_seen || !is_http_token(parameter) || parameter.eq_ignore_ascii_case("q") {
                return None;
            }
            if parameter_names
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(parameter))
            {
                return None;
            }
            parameter_names.push((*parameter).to_owned());
        }
    }
    Some((media_type, quality))
}

fn accepts_mcp_media_types(value: &str) -> bool {
    let Some(parts) = split_quoted(value, b',') else {
        return false;
    };
    let mut json_quality = None;
    let mut event_quality = None;
    for part in parts {
        let Some((media_type, quality)) = parse_accept_item(part) else {
            return false;
        };
        if media_type.eq_ignore_ascii_case("application/json") {
            if json_quality.is_some() {
                return false;
            }
            json_quality = Some(quality);
        } else if media_type.eq_ignore_ascii_case("text/event-stream") {
            if event_quality.is_some() {
                return false;
            }
            event_quality = Some(quality);
        }
    }
    matches!(json_quality, Some(quality) if quality > 0)
        && matches!(event_quality, Some(quality) if quality > 0)
}

fn acquire_dispatch_permit(
    slots: &Arc<Semaphore>,
) -> Result<tokio::sync::OwnedSemaphorePermit, WireResponse> {
    slots.clone().try_acquire_owned().map_err(|_| error(503))
}

fn exactly_one<'a>(
    headers: &'a hyper::HeaderMap,
    name: hyper::header::HeaderName,
) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(first)
}

#[derive(Clone)]
enum ServerSource {
    Legacy {
        archive: ArchiveStore,
        state_path: PathBuf,
        index_path: PathBuf,
    },
    #[cfg(feature = "vault-mcp")]
    Vault(Arc<VaultCaptureStore>),
}

#[derive(Clone)]
struct ServerContext {
    source: ServerSource,
    security: SecurityGate,
    active: Arc<AtomicBool>,
    dispatch_slots: Arc<Semaphore>,
    /// One request touches the encrypted stores at a time. Opening them takes a short exclusive schema lock
    /// that fails fast with `Busy`; without this queue, parallel tool calls from one client (normal for
    /// real clients) raced and all but one received HTTP 503.
    serial: Arc<tokio::sync::Mutex<()>>,
}

async fn handle(
    request: Request<Incoming>,
    context: ServerContext,
) -> Result<WireResponse, std::convert::Infallible> {
    let outcome = tokio::time::timeout(IO_DEADLINE, handle_bounded(request, context)).await;
    Ok(match outcome {
        Ok(response) => response,
        Err(_) => error(408),
    })
}

async fn handle_bounded(request: Request<Incoming>, ctx: ServerContext) -> WireResponse {
    if !ctx.active.load(Ordering::Acquire) {
        return error(503);
    }
    if request.uri().path() != "/mcp" || request.uri().query().is_some() {
        return error(404);
    }
    // Reject DNS rebinding and browser-origin requests before reading secrets.
    // Browser clients are intentionally unsupported in 3C.
    if exactly_one(request.headers(), HOST) != Some("127.0.0.1:43731")
        || request.headers().contains_key(ORIGIN)
        || request.headers().contains_key("forwarded")
        || request.headers().contains_key("x-forwarded-host")
        || request.headers().contains_key("x-forwarded-proto")
        || request.headers().contains_key("x-forwarded-for")
        || request.headers().contains_key("x-real-ip")
    {
        return error(403);
    }
    if request.method() != Method::POST {
        return error(405);
    }
    if request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v != "application/json" && v != "application/json; charset=utf-8")
        || exactly_one(request.headers(), CONTENT_TYPE).is_none()
    {
        return error(415);
    }
    let accepts = exactly_one(request.headers(), ACCEPT).unwrap_or_default();
    if !accepts_mcp_media_types(accepts) {
        return error(406);
    }
    if request.headers().get_all(CONTENT_LENGTH).iter().count() > 1
        || request
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .is_some_and(|length| length > BODY_LIMIT)
    {
        return error(413);
    }
    let authorization = match exactly_one(request.headers(), AUTHORIZATION) {
        Some(value)
            if value.starts_with("Bearer ")
                && value.len() == 7 + crate::CREDENTIAL_PREFIX.len() + 64 =>
        {
            Zeroizing::new(value[7..].to_string())
        }
        _ => return error(401),
    };
    let version = exactly_one(
        request.headers(),
        hyper::header::HeaderName::from_static("mcp-protocol-version"),
    )
    // Without a version header, Streamable HTTP clients use the earliest
    // handshake revision (2025-03-26). initialize negotiates independently.
    .unwrap_or(mcp::FIRST_HTTP_LEGACY)
    .to_string();
    let method = exactly_one(
        request.headers(),
        hyper::header::HeaderName::from_static("mcp-method"),
    )
    .map(str::to_owned);
    let name = exactly_one(
        request.headers(),
        hyper::header::HeaderName::from_static("mcp-name"),
    )
    .map(str::to_owned);
    let mut body = request.into_body();
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(_) => return error(400),
        };
        if let Ok(data) = frame.into_data() {
            if data.len() > BODY_LIMIT.saturating_sub(bytes.len()) {
                return error(413);
            }
            bytes.extend_from_slice(&data);
        }
    }
    let parsed = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(value) if !value.is_array() => value,
        _ => {
            return wire(
                400,
                Some(serde_json::json!({
                    "jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}
                })),
            )
        }
    };
    dispatch_parsed_request(ctx, authorization, parsed, version, method, name).await
}

async fn dispatch_parsed_request(
    ctx: ServerContext,
    authorization: Zeroizing<String>,
    parsed: serde_json::Value,
    version: String,
    method: Option<String>,
    name: Option<String>,
) -> WireResponse {
    // Pin the original unlock epoch for the entire HTTP exchange. Merely
    // checking 'unlocked' at the end could accept a lock-then-unlock race.
    let lease = match ctx.security.require_unlocked() {
        Ok(lease) => lease,
        Err(_) => return error(403),
    };
    if !ctx.active.load(Ordering::Acquire) {
        return error(503);
    }
    // Wait for our turn (bounded by the request deadline). The guard is moved into the blocking worker so a
    // disconnect cannot release the turn while the store connections are still open.
    let serial_turn = ctx.serial.clone().lock_owned().await;
    // Unlike a per-connection limit, this permit is owned by the blocking
    // worker. Disconnects and Tokio deadlines cannot release it prematurely.
    let dispatch_permit = match acquire_dispatch_permit(&ctx.dispatch_slots) {
        Ok(permit) => permit,
        Err(response) => return response,
    };
    let post_security = ctx.security.clone();
    let active = ctx.active.clone();
    let active_for_task = active.clone();
    let task = tokio::task::spawn_blocking(move || {
        let _dispatch_permit = dispatch_permit;
        let _serial_turn = serial_turn;
        if !active_for_task.load(Ordering::Acquire) {
            return error(503);
        }
        if ctx.security.validate_lease(lease).is_err() {
            return error(403);
        }
        let reply: McpReply = match &ctx.source {
            ServerSource::Legacy {
                archive,
                state_path,
                index_path,
            } => {
                let state = match StateStore::open(state_path) {
                    Ok(value) => value,
                    Err(_) => return error(503),
                };
                // MP-9C: the plaintext legacy archive has no per-source
                // visibility gate; refuse every legacy MCP read while any
                // tombstone exists (or the policy is unreadable).
                if !matches!(state.has_source_redactions(), Ok(false)) {
                    return error(503);
                }
                let index = match IndexStore::open(index_path) {
                    Ok(value) => value,
                    Err(_) => return error(503),
                };
                let core = crate::IntegrationCore::new(archive, &index, &ctx.security);
                mcp::dispatch(
                    &core,
                    &state,
                    &authorization,
                    parsed,
                    &version,
                    method.as_deref(),
                    name.as_deref(),
                )
            }
            #[cfg(feature = "vault-mcp")]
            ServerSource::Vault(archive) => {
                // MP-9C: hold the shared privacy disclosure fence (before any
                // state/index connection) until the reply body is serialized.
                let _privacy = match archive.disclosure_fence() {
                    Ok(guard) => guard,
                    Err(_) => return error(503),
                };
                let state = match archive.open_state_store() {
                    Ok(value) => value,
                    Err(_) => return error(503),
                };
                let index = match archive.open_index_store() {
                    Ok(value) => value,
                    Err(_) => return error(503),
                };
                let core = crate::IntegrationCore::new_vault(archive, &index, &ctx.security);
                mcp::dispatch(
                    &core,
                    &state,
                    &authorization,
                    parsed,
                    &version,
                    method.as_deref(),
                    name.as_deref(),
                )
            }
        };
        if !active_for_task.load(Ordering::Acquire) || ctx.security.validate_lease(lease).is_err() {
            return error(403);
        }
        wire(reply.status, reply.body)
    });
    match task.await {
        Ok(response)
            if ctx_active_postcondition(&active) && post_security.validate_lease(lease).is_ok() =>
        {
            response
        }
        _ => error(503),
    }
}

fn ctx_active_postcondition(active: &Arc<AtomicBool>) -> bool {
    active.load(Ordering::Acquire)
}

/// A private, explicitly started loopback listener. Drop aborts admissions
/// immediately; lock epoch checks still prevent any in-flight disclosure.
pub struct LocalMcpServer {
    active: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl LocalMcpServer {
    pub async fn start(
        archive: ArchiveStore,
        security: SecurityGate,
        state_path: PathBuf,
        index_path: PathBuf,
    ) -> std::io::Result<Self> {
        Self::start_source(
            ServerSource::Legacy {
                archive,
                state_path,
                index_path,
            },
            security,
        )
        .await
    }

    #[cfg(feature = "vault-mcp")]
    pub async fn start_vault(
        archive: Arc<VaultCaptureStore>,
        security: SecurityGate,
    ) -> std::io::Result<Self> {
        Self::start_source(ServerSource::Vault(archive), security).await
    }

    async fn start_source(source: ServerSource, security: SecurityGate) -> std::io::Result<Self> {
        security.require_unlocked().map_err(std::io::Error::other)?;
        // Fixed loopback-only port gives local MCP clients a stable endpoint.
        // Occupied port fails; it NEVER falls back to a public interface.
        let listener = TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::LOCALHOST,
            MCP_PORT,
        )))
        .await?;
        let active = Arc::new(AtomicBool::new(true));
        let shutdown = Arc::new(Notify::new());
        let gate = security.clone();
        let context = ServerContext {
            source,
            security,
            active: active.clone(),
            dispatch_slots: Arc::new(Semaphore::new(MAX_DISPATCH_TASKS)),
            serial: Arc::new(tokio::sync::Mutex::new(())),
        };
        let keep_active = active.clone();
        let stop = shutdown.clone();
        let task = tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
            loop {
                tokio::select! {
                    biased;
                    _ = stop.notified() => break,
                    connection = listener.accept() => {
                        let (socket, peer) = match connection {
                            Ok(pair) => pair,
                            Err(_) => break,
                        };
                        if !keep_active.load(Ordering::Acquire) || gate.require_unlocked().is_err() {
                            break;
                        }
                        if !peer.ip().is_loopback() {
                            continue;
                        }
                        let permit = match slots.clone().try_acquire_owned() {
                            Ok(permit) => permit,
                            Err(_) => continue,
                        };
                        let context = context.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            let svc = service_fn(move |request| handle(request, context.clone()));
                            let connection = hyper::server::conn::http1::Builder::new()
                                .keep_alive(false)
                                .serve_connection(TokioIo::new(socket), svc);
                            let _ = tokio::time::timeout(IO_DEADLINE, connection).await;
                        });
                    }
                }
            }
            keep_active.store(false, Ordering::Release);
        });
        Ok(Self {
            active,
            shutdown,
            task,
        })
    }

    pub fn endpoint(&self) -> &'static str {
        "http://127.0.0.1:43731/mcp"
    }

    pub fn stop(&self) {
        self.active.store(false, Ordering::Release);
        self.shutdown.notify_one();
        self.task.abort();
    }
}
impl Drop for LocalMcpServer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn accept_media_types_require_both_explicit_positive_qualities() {
        assert!(accepts_mcp_media_types(
            "application/json, text/event-stream"
        ));
        assert!(accepts_mcp_media_types(
            "application/json;q=0.8, text/event-stream;q=0.2"
        ));
        assert!(accepts_mcp_media_types(
            "APPLICATION/JSON; CHARSET=\"utf-8\"; Q=0.9, Text/Event-Stream;Q=0.1"
        ));
        assert!(accepts_mcp_media_types(
            "application/json; profile=\"a,b;c\"; q=0.8, text/event-stream; q=0.2"
        ));
    }

    #[test]
    fn accept_media_types_reject_zero_and_invalid_quality_values() {
        for value in [
            "application/json;q=0, text/event-stream",
            "application/json, text/event-stream;q=0",
            "application/json;q=0, text/event-stream;q=0",
            "application/json;q=1.001, text/event-stream",
            "application/json;q=0.0000, text/event-stream",
            "application/json;q=.5, text/event-stream",
            "application/json;q=2, text/event-stream",
            "application/json;q=0.5.0, text/event-stream",
        ] {
            assert!(!accepts_mcp_media_types(value), "{value}");
        }
    }

    #[test]
    fn accept_media_types_reject_duplicate_and_malformed_parameters() {
        for value in [
            "application/json;q=0.5;Q=0.6, text/event-stream",
            "application/json; charset, text/event-stream",
            "application/json; charset=\"unterminated, text/event-stream",
            "application/json; charset=\"bad\n\", text/event-stream",
            "application/json; charset=utf-8; CHARSET=utf-16, text/event-stream",
            "application/json;q=0.5; q, text/event-stream",
        ] {
            assert!(!accepts_mcp_media_types(value), "{value}");
        }
    }

    #[test]
    fn accept_media_types_reject_substring_and_wildcard_spoofing() {
        for value in [
            "application/json-evil, text/event-stream",
            "application/json, text/event-stream-evil",
            "application/jsonian, text/event-stream-ish",
            "application/*, text/*",
            "*/*",
            "application/json;q=1, */*",
        ] {
            assert!(!accepts_mcp_media_types(value), "{value}");
        }
    }

    fn gated_worker(
        slots: &Arc<Semaphore>,
    ) -> (
        tokio::task::JoinHandle<()>,
        mpsc::Receiver<()>,
        mpsc::Sender<()>,
    ) {
        let permit = match acquire_dispatch_permit(slots) {
            Ok(permit) => permit,
            Err(_) => panic!("test worker must acquire a dispatch permit"),
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = tokio::task::spawn_blocking(move || {
            let _dispatch_permit = permit;
            started_tx
                .send(())
                .expect("test observer remains connected");
            release_rx.recv().expect("test releases the worker");
        });
        (worker, started_rx, release_tx)
    }

    #[tokio::test]
    async fn four_dispatch_workers_exhaust_capacity_and_recovery_admits_next_request() {
        let slots = Arc::new(Semaphore::new(MAX_DISPATCH_TASKS));
        let mut workers = Vec::new();
        let mut started = Vec::new();
        let mut releases = Vec::new();
        for _ in 0..MAX_DISPATCH_TASKS {
            let (worker, started_rx, release_tx) = gated_worker(&slots);
            workers.push(worker);
            started.push(started_rx);
            releases.push(release_tx);
        }
        for started_rx in started {
            started_rx.recv().expect("each blocking worker started");
        }
        assert_eq!(slots.available_permits(), 0);
        match acquire_dispatch_permit(&slots) {
            Err(response) => assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE),
            Ok(permit) => {
                drop(permit);
                panic!("fifth request must fail closed with HTTP 503");
            }
        }

        for release in releases {
            release.send(()).expect("release blocking worker");
        }
        for worker in workers {
            worker.await.expect("blocking worker exits cleanly");
        }
        assert_eq!(slots.available_permits(), MAX_DISPATCH_TASKS);
        let subsequent = match acquire_dispatch_permit(&slots) {
            Ok(permit) => permit,
            Err(_) => panic!("a subsequent request must succeed after capacity returns"),
        };
        drop(subsequent);
    }

    #[tokio::test]
    async fn http_timeout_does_not_release_permit_while_blocking_worker_is_active() {
        let slots = Arc::new(Semaphore::new(1));
        let (worker, started_rx, release_tx) = gated_worker(&slots);
        started_rx.recv().expect("blocking worker started");

        // Zero-duration timeout is deterministic; the worker remains blocked
        // on its channel and the timed-out request future drops its JoinHandle.
        let timed_out = tokio::time::timeout(Duration::ZERO, async move {
            worker.await.expect("blocking worker exits cleanly");
        })
        .await;
        assert!(timed_out.is_err());
        assert_eq!(slots.available_permits(), 0);
        match acquire_dispatch_permit(&slots) {
            Err(response) => assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE),
            Ok(permit) => {
                drop(permit);
                panic!("HTTP timeout must not free an active worker slot");
            }
        }

        release_tx.send(()).expect("release blocking worker");
        let recovered = slots
            .clone()
            .acquire_owned()
            .await
            .expect("permit returns after the worker exits");
        drop(recovered);
    }

    #[tokio::test]
    async fn disconnected_client_does_not_release_permit_before_worker_exit() {
        let slots = Arc::new(Semaphore::new(1));
        let (worker, started_rx, release_tx) = gated_worker(&slots);
        started_rx.recv().expect("blocking worker started");
        drop(worker); // Dropping the HTTP request's JoinHandle models disconnect.
        assert_eq!(slots.available_permits(), 0);
        match acquire_dispatch_permit(&slots) {
            Err(response) => assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE),
            Ok(permit) => {
                drop(permit);
                panic!("disconnect must not free an active worker slot");
            }
        }

        release_tx.send(()).expect("release blocking worker");
        let recovered = slots
            .clone()
            .acquire_owned()
            .await
            .expect("permit returns only after the worker exits");
        drop(recovered);
    }

    struct ConcurrencyFixture {
        _temp: tempfile::TempDir,
        ctx: ServerContext,
        state_path: PathBuf,
        security: SecurityGate,
        client_a_id: String,
        token_a: String,
        source_a: String,
        client_b_id: String,
        token_b: String,
        source_b: String,
    }

    fn setup_concurrency_fixture() -> ConcurrencyFixture {
        use mind_place_archive::{Capture, Message};
        use mind_place_state::ClientCapability;
        let temp = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(temp.path().join("archive-v1")).unwrap();
        let state_path = temp.path().join("state.sqlite3");
        let index_path = temp.path().join("index.sqlite3");
        let state = StateStore::open(&state_path).unwrap();
        let index = IndexStore::open(&index_path).unwrap();
        for (cid, title, text) in [
            (
                "11111111-1111-4111-8111-111111111111",
                "Alpha chat",
                "alpha-secret-phrase",
            ),
            (
                "22222222-2222-4222-8222-222222222222",
                "Beta chat",
                "beta-secret-phrase",
            ),
        ] {
            archive
                .save(Capture {
                    schema_version: 1,
                    provider: "chatgpt".into(),
                    conversation_id: cid.into(),
                    source_url: format!("https://chatgpt.com/c/{cid}"),
                    title: title.into(),
                    conversation_created_at: None,
                    conversation_updated_at: None,
                    captured_at: "2026-11-24T00:00:00Z".into(),
                    capture_method: "manual-dom".into(),
                    completeness: "partial".into(),
                    warnings: vec![],
                    messages: vec![Message {
                        id: format!("m-{cid}"),
                        role: "user".into(),
                        text: text.into(),
                        format: "plain".into(),
                        authored_at: None,
                        model: None,
                        attachments: vec![],
                    }],
                })
                .unwrap();
        }
        index.sync(&archive, &state).unwrap();
        let security = SecurityGate::open(temp.path().join("security.json")).unwrap();
        security.initialize(b"concurrency-test-secret-789").unwrap();
        let core = crate::IntegrationCore::new(&archive, &index, &security);
        let find_source = |q: &str| {
            let batch = core
                .execute_batch(
                    &crate::ToolBatch {
                        calls: vec![crate::ToolCall {
                            name: crate::ToolName::MemorySearch,
                            arguments: serde_json::json!({"queries": [q]}),
                        }],
                    },
                    || Ok(()),
                )
                .unwrap();
            batch.results[0].result["hits"][0]["source_id"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let source_a = find_source("alpha-secret-phrase");
        let source_b = find_source("beta-secret-phrase");
        let auth = crate::ClientAuthority::new(&state, &security);
        let a = auth.issue("Client A", &[ClientCapability::Read]).unwrap();
        let b = auth.issue("Client B", &[ClientCapability::Read]).unwrap();
        auth.grant_read_sources(&a.client.id, std::slice::from_ref(&source_a))
            .unwrap();
        auth.grant_read_sources(&b.client.id, std::slice::from_ref(&source_b))
            .unwrap();
        let ctx = ServerContext {
            source: ServerSource::Legacy {
                archive,
                state_path: state_path.clone(),
                index_path,
            },
            security: security.clone(),
            active: Arc::new(AtomicBool::new(true)),
            dispatch_slots: Arc::new(Semaphore::new(MAX_DISPATCH_TASKS)),
            serial: Arc::new(tokio::sync::Mutex::new(())),
        };
        let token_a = a.credential().to_string();
        let token_b = b.credential().to_string();
        ConcurrencyFixture {
            _temp: temp,
            ctx,
            state_path,
            security,
            client_a_id: a.client.id,
            token_a,
            source_a,
            client_b_id: b.client.id,
            token_b,
            source_b,
        }
    }

    async fn read_wire_body(response: WireResponse) -> (StatusCode, String) {
        let status = response.status();
        let collected = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&collected).into_owned())
    }

    fn search_rpc(query: &str) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "memory_search",
                "arguments": { "queries": [query] }
            }
        })
    }

    #[tokio::test]
    async fn queued_requests_fail_closed_when_locked_before_turn() {
        let f = setup_concurrency_fixture();
        let guard = f.ctx.serial.clone().lock_owned().await;
        let tasks: Vec<_> = (0..4)
            .map(|_| {
                tokio::spawn(dispatch_parsed_request(
                    f.ctx.clone(),
                    Zeroizing::new(f.token_a.clone()),
                    search_rpc("alpha-secret-phrase"),
                    mcp::CLAUDE_LEGACY.to_string(),
                    None,
                    None,
                ))
            })
            .collect();
        tokio::task::yield_now().await;
        f.security.lock();
        drop(guard);
        for task in tasks {
            let (status, body) = read_wire_body(task.await.unwrap()).await;
            assert!(
                status == StatusCode::FORBIDDEN || status == StatusCode::SERVICE_UNAVAILABLE,
                "locked queued request returned {status}"
            );
            assert!(!body.contains("alpha-secret-phrase"));
            assert!(!body.contains(&f.source_a));
        }
        assert_eq!(f.ctx.dispatch_slots.available_permits(), MAX_DISPATCH_TASKS);
    }

    #[tokio::test]
    async fn queued_requests_fail_closed_across_lock_and_unlock_epoch_change() {
        let f = setup_concurrency_fixture();
        let guard = f.ctx.serial.clone().lock_owned().await;
        let stale_tasks: Vec<_> = (0..3)
            .map(|_| {
                tokio::spawn(dispatch_parsed_request(
                    f.ctx.clone(),
                    Zeroizing::new(f.token_a.clone()),
                    search_rpc("alpha-secret-phrase"),
                    mcp::CLAUDE_LEGACY.to_string(),
                    None,
                    None,
                ))
            })
            .collect();
        tokio::task::yield_now().await;
        f.security.lock();
        f.security.unlock(b"concurrency-test-secret-789").unwrap();
        drop(guard);
        for task in stale_tasks {
            let (status, body) = read_wire_body(task.await.unwrap()).await;
            assert!(
                status == StatusCode::FORBIDDEN || status == StatusCode::SERVICE_UNAVAILABLE,
                "stale-epoch queued request returned {status}"
            );
            assert!(!body.contains("alpha-secret-phrase"));
        }
        let fresh = dispatch_parsed_request(
            f.ctx.clone(),
            Zeroizing::new(f.token_a.clone()),
            search_rpc("alpha-secret-phrase"),
            mcp::CLAUDE_LEGACY.to_string(),
            None,
            None,
        )
        .await;
        let (status, body) = read_wire_body(fresh).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("alpha-secret-phrase"));
        assert_eq!(f.ctx.dispatch_slots.available_permits(), MAX_DISPATCH_TASKS);
    }

    #[tokio::test]
    async fn grant_revocation_during_queueing_denies_revoked_grant_without_affecting_disjoint_client(
    ) {
        let f = setup_concurrency_fixture();
        let guard = f.ctx.serial.clone().lock_owned().await;
        let task_a = tokio::spawn(dispatch_parsed_request(
            f.ctx.clone(),
            Zeroizing::new(f.token_a.clone()),
            search_rpc("alpha-secret-phrase"),
            mcp::CLAUDE_LEGACY.to_string(),
            None,
            None,
        ));
        let task_b = tokio::spawn(dispatch_parsed_request(
            f.ctx.clone(),
            Zeroizing::new(f.token_b.clone()),
            search_rpc("beta-secret-phrase"),
            mcp::CLAUDE_LEGACY.to_string(),
            None,
            None,
        ));
        tokio::task::yield_now().await;
        {
            let state = StateStore::open(&f.state_path).unwrap();
            crate::ClientAuthority::new(&state, &f.security)
                .revoke_read_sources(&f.client_a_id, std::slice::from_ref(&f.source_a))
                .unwrap();
        }
        drop(guard);
        let (status_a, body_a) = read_wire_body(task_a.await.unwrap()).await;
        assert_eq!(status_a, StatusCode::OK);
        assert!(!body_a.contains("alpha-secret-phrase"));
        assert!(!body_a.contains(&f.source_a));
        let (status_b, body_b) = read_wire_body(task_b.await.unwrap()).await;
        assert_eq!(status_b, StatusCode::OK);
        assert!(body_b.contains("beta-secret-phrase"));
        assert!(!body_b.contains("alpha-secret-phrase"));
    }

    #[tokio::test]
    async fn whole_client_revocation_during_queueing_refuses_revoked_client() {
        let f = setup_concurrency_fixture();
        let guard = f.ctx.serial.clone().lock_owned().await;
        let task_b = tokio::spawn(dispatch_parsed_request(
            f.ctx.clone(),
            Zeroizing::new(f.token_b.clone()),
            search_rpc("beta-secret-phrase"),
            mcp::CLAUDE_LEGACY.to_string(),
            None,
            None,
        ));
        let task_a = tokio::spawn(dispatch_parsed_request(
            f.ctx.clone(),
            Zeroizing::new(f.token_a.clone()),
            search_rpc("alpha-secret-phrase"),
            mcp::CLAUDE_LEGACY.to_string(),
            None,
            None,
        ));
        tokio::task::yield_now().await;
        {
            let state = StateStore::open(&f.state_path).unwrap();
            crate::ClientAuthority::new(&state, &f.security)
                .revoke(&f.client_b_id)
                .unwrap();
        }
        drop(guard);
        let (status_b, body_b) = read_wire_body(task_b.await.unwrap()).await;
        assert!(
            status_b == StatusCode::UNAUTHORIZED || status_b == StatusCode::FORBIDDEN,
            "revoked queued client returned {status_b}"
        );
        assert!(!body_b.contains("beta-secret-phrase"));
        assert!(!body_b.contains(&f.source_b));
        let (status_a, body_a) = read_wire_body(task_a.await.unwrap()).await;
        assert_eq!(status_a, StatusCode::OK);
        assert!(body_a.contains("alpha-secret-phrase"));
        assert_eq!(f.ctx.dispatch_slots.available_permits(), MAX_DISPATCH_TASKS);
    }

    #[tokio::test]
    async fn queued_timeout_and_disconnect_preserve_bounded_permits_and_recover() {
        let f = setup_concurrency_fixture();
        let guard = f.ctx.serial.clone().lock_owned().await;
        let timed_out_req = tokio::spawn(tokio::time::timeout(
            Duration::from_millis(10),
            dispatch_parsed_request(
                f.ctx.clone(),
                Zeroizing::new(f.token_a.clone()),
                search_rpc("alpha-secret-phrase"),
                mcp::CLAUDE_LEGACY.to_string(),
                None,
                None,
            ),
        ));
        let disconnected_req = tokio::spawn(dispatch_parsed_request(
            f.ctx.clone(),
            Zeroizing::new(f.token_b.clone()),
            search_rpc("beta-secret-phrase"),
            mcp::CLAUDE_LEGACY.to_string(),
            None,
            None,
        ));
        assert!(timed_out_req.await.unwrap().is_err());
        disconnected_req.abort();
        let _ = disconnected_req.await;
        drop(guard);
        assert_eq!(f.ctx.dispatch_slots.available_permits(), MAX_DISPATCH_TASKS);
        assert!(f.ctx.serial.try_lock().is_ok());
        let recovered = dispatch_parsed_request(
            f.ctx.clone(),
            Zeroizing::new(f.token_a.clone()),
            search_rpc("alpha-secret-phrase"),
            mcp::CLAUDE_LEGACY.to_string(),
            None,
            None,
        )
        .await;
        let (status, body) = read_wire_body(recovered).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("alpha-secret-phrase"));
    }
}
