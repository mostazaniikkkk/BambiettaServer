//! Transport: classic TCP and WebSocket share the same protocol byte stream.
//!
//! Over WebSocket, binary messages from the client are treated as a byte *stream*
//! (exactly what would travel over TCP: frames with a VarInt length prefix). A message
//! may contain half a packet or several packets. The server sends one or more complete
//! packets per binary message.

pub mod listener;
pub mod tls;

use crate::protocol::buf::{peek_varint, varint_len, Writer};
use anyhow::{bail, ensure, Context, Result};
use bytes::{Buf, BytesMut};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use futures_util::{stream::SplitSink, stream::SplitStream, SinkExt, StreamExt};
use std::io::{Read, Write};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

const MAX_FRAME: usize = 2 * 1024 * 1024;
const MAX_UNCOMPRESSED: usize = 8 * 1024 * 1024;
/// Size beyond which the writer stops batching packets into a single send.
const COALESCE_LIMIT: usize = 256 * 1024;

/// Any byte stream (plain TCP or TLS) for the WebSocket.
pub trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Io for T {}
pub type BoxIo = Box<dyn Io>;

type Ws = WebSocketStream<BoxIo>;

/// Error when exceeding the packets-per-second limit.
#[derive(Debug)]
pub struct RateLimited;

impl std::fmt::Display for RateLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("demasiados paquetes por segundo")
    }
}

impl std::error::Error for RateLimited {}

pub enum RawIn {
    Tcp(OwnedReadHalf),
    Ws(SplitStream<Ws>),
}

pub enum RawOut {
    Tcp(OwnedWriteHalf),
    Ws(SplitSink<Ws, Message>),
}

/// Messages to a connection's writer task.
pub enum Out {
    /// Unframed packet body (VarInt id + data).
    Packet(Vec<u8>),
    /// Enables compression for subsequent packets.
    Compression(i32),
    /// Encrypts everything sent from now on (AES-128/CFB8 with this shared secret).
    Encrypt([u8; 16]),
    /// Closes the connection after flushing what is pending.
    Close,
}

/// AES-128 in CFB8 mode, the protocol's encryption once the login is authenticated (the
/// shared secret is both the key and the initial vector). One direction per instance.
pub struct Cfb8 {
    aes: aes::Aes128,
    iv: [u8; 16],
}

impl Cfb8 {
    pub fn new(secret: [u8; 16]) -> Self {
        use aes::cipher::KeyInit;
        Self { aes: aes::Aes128::new(&secret.into()), iv: secret }
    }

    fn keystream(&self) -> u8 {
        use aes::cipher::BlockEncrypt;
        let mut block = self.iv.into();
        self.aes.encrypt_block(&mut block);
        block[0]
    }

    fn shift(&mut self, c: u8) {
        self.iv.copy_within(1.., 0);
        self.iv[15] = c;
    }

    pub fn encrypt(&mut self, data: &mut [u8]) {
        for b in data {
            *b ^= self.keystream();
            self.shift(*b);
        }
    }

    pub fn decrypt(&mut self, data: &mut [u8]) {
        for b in data {
            let c = *b;
            *b ^= self.keystream();
            self.shift(c);
        }
    }
}

pub type Tx = mpsc::UnboundedSender<Out>;

pub struct Conn {
    pub reader: PacketReader,
    pub tx: Tx,
    pub peer: SocketAddr,
    pub transport: &'static str,
}

impl Conn {
    /// `transport`: "tcp", "ws" or "wss".
    pub fn new(raw_in: RawIn, raw_out: RawOut, peer: SocketAddr, transport: &'static str) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(write_loop(raw_out, rx));
        let reader = PacketReader { raw: raw_in, buf: BytesMut::with_capacity(4096), compression: None, cipher: None, max_per_second: 0, window: (std::time::Instant::now(), 0) };
        Self { reader, tx, peer, transport }
    }

    pub fn send(&self, body: Vec<u8>) {
        let _ = self.tx.send(Out::Packet(body));
    }
}

pub struct PacketReader {
    raw: RawIn,
    buf: BytesMut,
    pub compression: Option<i32>,
    cipher: Option<Cfb8>,
    /// Allowed packets per second (0 = unlimited).
    pub max_per_second: u32,
    window: (std::time::Instant, u32),
}

impl PacketReader {
    /// Decrypts everything received from now on, including what is already buffered.
    pub fn decrypt(&mut self, secret: [u8; 16]) {
        let mut c = Cfb8::new(secret);
        c.decrypt(&mut self.buf);
        self.cipher = Some(c);
    }

    /// Reads more bytes from the transport. Returns `false` if the connection ended.
    async fn fill(&mut self) -> Result<bool> {
        let start = self.buf.len();
        let more = self.fill_raw().await?;
        if let Some(c) = &mut self.cipher {
            c.decrypt(&mut self.buf[start..]);
        }
        Ok(more)
    }

    async fn fill_raw(&mut self) -> Result<bool> {
        match &mut self.raw {
            RawIn::Tcp(s) => Ok(s.read_buf(&mut self.buf).await? > 0),
            RawIn::Ws(s) => loop {
                match s.next().await {
                    None => return Ok(false),
                    Some(Err(e)) => return Err(e.into()),
                    Some(Ok(Message::Binary(b))) => {
                        self.buf.extend_from_slice(&b);
                        return Ok(true);
                    }
                    Some(Ok(Message::Close(_))) => return Ok(false),
                    // Ping/Pong are answered by tungstenite; text is not part of the protocol.
                    Some(Ok(_)) => continue,
                }
            },
        }
    }

    /// Next packet body (id + data), already decompressed. `None` on close.
    pub async fn next(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            if let Some((len, n)) = peek_varint(&self.buf)? {
                ensure!(len >= 0 && (len as usize) <= MAX_FRAME, "trama inválida ({len} bytes)");
                if self.buf.len() >= n + len as usize {
                    if self.max_per_second > 0 {
                        let now = std::time::Instant::now();
                        if now.duration_since(self.window.0) >= std::time::Duration::from_secs(1) {
                            self.window = (now, 0);
                        }
                        self.window.1 += 1;
                        if self.window.1 > self.max_per_second {
                            return Err(RateLimited.into());
                        }
                    }
                    self.buf.advance(n);
                    let frame = self.buf.split_to(len as usize);
                    return self.decode(&frame).map(Some);
                }
            }
            if !self.fill().await? {
                return Ok(None);
            }
        }
    }

    fn decode(&self, frame: &[u8]) -> Result<Vec<u8>> {
        if self.compression.is_none() {
            return Ok(frame.to_vec());
        }
        let (data_len, n) = peek_varint(frame)?.context("trama comprimida truncada")?;
        if data_len == 0 {
            return Ok(frame[n..].to_vec());
        }
        ensure!(data_len > 0 && data_len as usize <= MAX_UNCOMPRESSED, "tamaño descomprimido inválido");
        let mut out = Vec::with_capacity(data_len as usize);
        ZlibDecoder::new(&frame[n..]).take(data_len as u64).read_to_end(&mut out)?;
        if out.len() != data_len as usize {
            bail!("tamaño descomprimido no coincide");
        }
        Ok(out)
    }
}

fn frame(body: &[u8], threshold: Option<i32>, out: &mut Vec<u8>) {
    match threshold {
        None => {
            out.varint(body.len() as i32);
            out.extend_from_slice(body);
        }
        Some(t) if (body.len() as i32) < t => {
            out.varint(body.len() as i32 + 1);
            out.u8(0);
            out.extend_from_slice(body);
        }
        Some(_) => {
            let mut enc = ZlibEncoder::new(Vec::with_capacity(body.len() / 2), Compression::fast());
            enc.write_all(body).unwrap();
            let compressed = enc.finish().unwrap();
            out.varint((varint_len(body.len() as i32) + compressed.len()) as i32);
            out.varint(body.len() as i32);
            out.extend_from_slice(&compressed);
        }
    }
}

async fn send(raw: &mut RawOut, batch: Vec<u8>) -> Result<()> {
    match raw {
        RawOut::Tcp(s) if !batch.is_empty() => Ok(s.write_all(&batch).await?),
        RawOut::Ws(s) if !batch.is_empty() => Ok(s.send(Message::Binary(batch)).await?),
        _ => Ok(()),
    }
}

async fn write_loop(mut raw: RawOut, mut rx: mpsc::UnboundedReceiver<Out>) {
    let mut threshold = None;
    let mut cipher: Option<Cfb8> = None;
    let mut closing = false;
    while let Some(first) = rx.recv().await {
        // Batch everything already queued to reduce messages/syscalls.
        let mut batch = Vec::new();
        let mut next = Some(first);
        while let Some(msg) = next.take() {
            match msg {
                Out::Packet(body) => frame(&body, threshold, &mut batch),
                Out::Compression(t) => threshold = Some(t),
                Out::Encrypt(secret) => {
                    // What is already batched goes out in clear, the rest encrypted.
                    if let Some(c) = &mut cipher {
                        c.encrypt(&mut batch);
                    }
                    if !batch.is_empty() && send(&mut raw, std::mem::take(&mut batch)).await.is_err() {
                        closing = true;
                        break;
                    }
                    cipher = Some(Cfb8::new(secret));
                }
                Out::Close => {
                    closing = true;
                    break;
                }
            }
            if batch.len() < COALESCE_LIMIT {
                next = rx.try_recv().ok();
            }
        }
        if let Some(c) = &mut cipher {
            c.encrypt(&mut batch);
        }
        let res = send(&mut raw, batch).await;
        if res.is_err() || closing {
            break;
        }
    }
    match &mut raw {
        RawOut::Tcp(s) => drop(s.shutdown().await),
        RawOut::Ws(s) => drop(s.close().await),
    }
}
