//! Connection acceptance: a single port serves vanilla TCP, `ws://` and `wss://`.
//!
//! The first bytes are peeked without consuming them:
//! - `0x16` (TLS "handshake" record): TLS is negotiated and HTTP/WebSocket is expected inside;
//! - `GET `: HTTP request (upgrade to WebSocket, or a status page if it isn't one);
//! - anything else: raw Minecraft protocol. A vanilla handshake never starts
//!   like that: its first byte is the length and the second the packet id 0x00.

use super::{BoxIo, Conn, RawIn, RawOut};
use crate::server::Server;
use anyhow::{bail, Result};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tracing::{debug, info, warn};

#[derive(Clone, Copy)]
pub enum Mode {
    /// Detects TCP, WebSocket and TLS automatically.
    Auto,
    /// WebSocket only (with or without TLS).
    WebSocket,
}

/// Per-IP connection limit (simultaneous and per time window).
#[derive(Default)]
pub struct IpLimiter {
    ips: Mutex<HashMap<IpAddr, (usize, Vec<Instant>)>>,
}

/// While alive it counts as an active connection for its IP.
pub struct IpGuard {
    limiter: Arc<IpLimiter>,
    ip: IpAddr,
}

impl Drop for IpGuard {
    fn drop(&mut self) {
        let mut ips = self.limiter.ips.lock().unwrap();
        if let Some(e) = ips.get_mut(&self.ip) {
            e.0 = e.0.saturating_sub(1);
            if e.0 == 0 && e.1.is_empty() {
                ips.remove(&self.ip);
            }
        }
    }
}

impl IpLimiter {
    fn admit(self: &Arc<Self>, ip: IpAddr, max_active: usize, max_per_10s: usize) -> Option<IpGuard> {
        let mut ips = self.ips.lock().unwrap();
        let e = ips.entry(ip).or_default();
        let now = Instant::now();
        e.1.retain(|t| now.duration_since(*t) < Duration::from_secs(10));
        if (max_active > 0 && e.0 >= max_active) || (max_per_10s > 0 && e.1.len() >= max_per_10s) {
            return None;
        }
        e.0 += 1;
        e.1.push(now);
        Some(IpGuard { limiter: self.clone(), ip })
    }
}

pub async fn serve(server: Arc<Server>, bind: SocketAddr, mode: Mode, tls: Option<TlsAcceptor>) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;
    let what = match (mode, tls.is_some()) {
        (Mode::Auto, true) => "TCP + WebSocket + WebSocket TLS",
        (Mode::Auto, false) => "TCP + WebSocket",
        (Mode::WebSocket, true) => "WebSocket + WebSocket TLS",
        (Mode::WebSocket, false) => "WebSocket",
    };
    info!("escuchando {what} en {bind}");
    let limiter = Arc::new(IpLimiter::default());
    loop {
        let (stream, peer) = listener.accept().await?;
        let cfg = &server.config;
        let local = peer.ip().is_loopback();
        let Some(guard) = (if local { Some(None) } else { limiter.admit(peer.ip(), cfg.max_connections_per_ip, cfg.connections_per_ip_per_10s).map(Some) }) else {
            debug!("{peer}: demasiadas conexiones desde esta IP");
            continue;
        };
        let server = server.clone();
        let tls = tls.clone();
        tokio::spawn(async move {
            let _guard = guard;
            if let Err(e) = accept(server, stream, peer, mode, tls).await {
                debug!("{peer}: {e:#}");
            }
        });
    }
}

async fn peek(stream: &TcpStream, n: usize) -> Result<Vec<u8>> {
    let mut head = vec![0u8; n];
    let got = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let got = stream.peek(&mut head).await?;
            if got == 0 || got >= n {
                return std::io::Result::Ok(got);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await??;
    head.truncate(got);
    Ok(head)
}

async fn accept(server: Arc<Server>, stream: TcpStream, peer: SocketAddr, mode: Mode, tls: Option<TlsAcceptor>) -> Result<()> {
    stream.set_nodelay(true)?;
    let head = peek(&stream, 4).await?;
    if head.is_empty() {
        return Ok(());
    }
    // TLS: only HTTP/WebSocket is spoken inside.
    if head[0] == 0x16 {
        let Some(tls) = tls else { bail!("intento de TLS sin certificado configurado") };
        let tls_stream = tokio::time::timeout(Duration::from_secs(10), tls.accept(stream)).await??;
        return http_or_ws(server, Box::new(tls_stream), peer, "wss").await;
    }
    if head.as_slice() == b"GET " || matches!(mode, Mode::WebSocket) {
        return http_or_ws(server, Box::new(stream), peer, "ws").await;
    }
    let (r, w) = stream.into_split();
    crate::server::session::handle(server, Conn::new(RawIn::Tcp(r), RawOut::Tcp(w), peer, "tcp")).await
}

/// Reads the HTTP header; if it is a WebSocket upgrade it completes it, otherwise it serves a
/// status page (useful for accepting a self-signed certificate from the browser).
async fn http_or_ws(server: Arc<Server>, mut io: BoxIo, peer: SocketAddr, transport: &'static str) -> Result<()> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    tokio::time::timeout(Duration::from_secs(10), async {
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = io.read(&mut chunk).await?;
            if n == 0 || buf.len() > 16 * 1024 {
                bail!("cabecera HTTP incompleta");
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Ok(())
    })
    .await??;
    let header = String::from_utf8_lossy(&buf).to_ascii_lowercase();
    if !header.contains("upgrade: websocket") {
        let body = format!(
            "<!doctype html><meta charset=utf-8><title>Bambietta</title><body style=\"font-family:sans-serif\">\
             <h1>Bambietta Server</h1><p>Minecraft {} por TCP y WebSocket. Conecta con <code>{}://</code> a este mismo puerto.</p>",
            crate::protocol::VERSION_NAME,
            transport
        );
        let res = format!("HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
        io.write_all(res.as_bytes()).await?;
        io.shutdown().await.ok();
        return Ok(());
    }

    let allowed = server.config.ws_allowed_origins.clone();
    let replay: BoxIo = Box::new(Prefixed { prefix: buf, pos: 0, inner: io });
    let ws = tokio_tungstenite::accept_hdr_async(replay, move |req: &Request, mut res: Response| {
        if !allowed.is_empty() {
            let origin = req.headers().get("origin").and_then(|o| o.to_str().ok()).unwrap_or("");
            if !allowed.iter().any(|a| a == "*" || a == origin) {
                warn!("{peer}: origen WebSocket rechazado: {origin:?}");
                let mut r = tokio_tungstenite::tungstenite::handshake::server::ErrorResponse::new(Some("origen no permitido".into()));
                *r.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::FORBIDDEN;
                return Err(r);
            }
        }
        // Browsers abort if they request a subprotocol and the server doesn't confirm it.
        if let Some(proto) = req.headers().get("sec-websocket-protocol").and_then(|p| p.to_str().ok()) {
            if let Some(first) = proto.split(',').map(str::trim).find(|p| !p.is_empty()) {
                if let Ok(v) = HeaderValue::from_str(first) {
                    res.headers_mut().insert("sec-websocket-protocol", v);
                }
            }
        }
        Ok(res)
    })
    .await?;
    let (sink, stream) = ws.split();
    crate::server::session::handle(server, Conn::new(RawIn::Ws(stream), RawOut::Ws(sink), peer, transport)).await
}

/// Stream that first returns bytes already read (the HTTP header) and then continues with the real one.
struct Prefixed {
    prefix: Vec<u8>,
    pos: usize,
    inner: BoxIo,
}

impl AsyncRead for Prefixed {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        if self.pos < self.prefix.len() {
            let n = (self.prefix.len() - self.pos).min(buf.remaining());
            let start = self.pos;
            buf.put_slice(&self.prefix[start..start + n]);
            self.pos += n;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for Prefixed {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_limits() {
        let l = Arc::new(IpLimiter::default());
        let ip: IpAddr = "203.0.113.7".parse().unwrap();
        let a = l.admit(ip, 2, 10).unwrap();
        let _b = l.admit(ip, 2, 10).unwrap();
        assert!(l.admit(ip, 2, 10).is_none(), "máximo 2 simultáneas");
        drop(a);
        assert!(l.admit(ip, 2, 10).is_some(), "al cerrar una, se libera el hueco");
        let other: IpAddr = "203.0.113.8".parse().unwrap();
        let guards: Vec<_> = (0..3).filter_map(|_| l.admit(other, 0, 3)).collect();
        assert_eq!(guards.len(), 3);
        assert!(l.admit(other, 0, 3).is_none(), "máximo 3 por 10 s");
    }
}
