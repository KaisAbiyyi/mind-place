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

type WireResponse = Response<Full<Bytes>>;

fn wire(status: u16, value: Option<serde_json::Value>) -> WireResponse {
    let bytes = value
        .and_then(|body| serde_json::to_vec(&body).ok())
        .unwrap_or_default();
    let (code, bytes) = if bytes.len() > MAX_WIRE_BYTES {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#.to_vec(),
        )
    } else {
        (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), bytes)
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

fn exactly_one<'a>(headers: &'a hyper::HeaderMap, name: hyper::header::HeaderName) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(first)
}

#[derive(Clone)]
struct ServerContext {
    archive: ArchiveStore,
    state_path: PathBuf,
    index_path: PathBuf,
    security: SecurityGate,
    active: Arc<AtomicBool>,
}

async fn handle(request: Request<Incoming>, context: ServerContext) -> Result<WireResponse, std::convert::Infallible> {
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
    {
        return error(403);
    }
    if request.method() != Method::POST {
        return error(405);
    }
    if request.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok())
        .is_none_or(|v| v != "application/json" && v != "application/json; charset=utf-8")
        || exactly_one(request.headers(), CONTENT_TYPE).is_none()
    {
        return error(415);
    }
    let accepts = exactly_one(request.headers(), ACCEPT).unwrap_or_default().to_ascii_lowercase();
    if !(accepts.contains("application/json") && accepts.contains("text/event-stream")) {
        return error(406);
    }
    if request.headers().get_all(CONTENT_LENGTH).iter().count() > 1
        || request.headers().get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .is_some_and(|length| length > BODY_LIMIT)
    {
        return error(413);
    }
    let authorization = match exactly_one(request.headers(), AUTHORIZATION) {
        Some(value) if value.starts_with("Bearer ") && value.len() == 7 + crate::CREDENTIAL_PREFIX.len() + 64 => {
            Zeroizing::new(value[7..].to_string())
        }
        _ => return error(401),
    };
    let version = exactly_one(request.headers(), hyper::header::HeaderName::from_static("mcp-protocol-version"))
        .unwrap_or(mcp::LEGACY).to_string();
    let method = exactly_one(request.headers(), hyper::header::HeaderName::from_static("mcp-method"))
        .map(str::to_owned);
    let name = exactly_one(request.headers(), hyper::header::HeaderName::from_static("mcp-name"))
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
        _ => return wire(400, Some(serde_json::json!({
            "jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}
        }))),
    };
    if !ctx.active.load(Ordering::Acquire) || ctx.security.require_unlocked().is_err() {
        return error(403);
    }

    let active = ctx.active.clone();
    let task = tokio::task::spawn_blocking(move || {
        if !active.load(Ordering::Acquire) { return error(503); }
        let state = match StateStore::open(ctx.state_path) {
            Ok(value) => value,
            Err(_) => return error(503),
        };
        let index = match IndexStore::open(ctx.index_path) {
            Ok(value) => value,
            Err(_) => return error(503),
        };
        let core = crate::IntegrationCore::new(&ctx.archive, &index, &ctx.security);
        let reply: McpReply = mcp::dispatch(
            &core, &state, &authorization, parsed, &version,
            method.as_deref(), name.as_deref(),
        );
        if !active.load(Ordering::Acquire) || ctx.security.require_unlocked().is_err() {
            return error(403);
        }
        wire(reply.status, reply.body)
    });
    match task.await {
        Ok(response) if ctx_active_postcondition(&active) => response,
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
        security.require_unlocked().map_err(std::io::Error::other)?;
        // Fixed loopback-only port gives local MCP clients a stable endpoint.
        // Occupied port fails; it NEVER falls back to a public interface.
        let listener = TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::LOCALHOST, MCP_PORT,
        ))).await?;
        let active = Arc::new(AtomicBool::new(true));
        let shutdown = Arc::new(Notify::new());
        let gate = security.clone();
        let context = ServerContext { archive, state_path, index_path, security, active: active.clone() };
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
        Ok(Self { active, shutdown, task })
    }

    pub fn endpoint(&self) -> &'static str { "http://127.0.0.1:43731/mcp" }

    pub fn stop(&self) {
        self.active.store(false, Ordering::Release);
        self.shutdown.notify_one();
        self.task.abort();
    }
}
impl Drop for LocalMcpServer {
    fn drop(&mut self) { self.stop(); }
}
