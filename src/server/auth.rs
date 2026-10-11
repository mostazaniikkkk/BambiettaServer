//! Online mode: the login is encrypted and checked against Mojang's session server, so only
//! owners of a Minecraft account can join with their name, and they get their real UUID and
//! skin.
//!
//! The flow (as in vanilla): the server sends its RSA public key and a random token, the
//! client answers with a shared secret (and the token) encrypted with that key, both sides
//! switch to AES-128/CFB8, and the server asks `sessionserver.mojang.com/hasJoined` whether
//! that player announced this session (the hash of the secret and the key) to Mojang.

use crate::net::Conn;
use crate::protocol::{packet, Reader, Writer};
use anyhow::{bail, ensure, Context, Result};
use rsa::pkcs8::EncodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::info;

const ENCRYPTION_REQUEST: i32 = 0x01;
const ENCRYPTION_RESPONSE: i32 = 0x01;
const SESSION_HOST: &str = "sessionserver.mojang.com";
const TIMEOUT: Duration = Duration::from_secs(10);

/// A profile property (`textures` carries the skin), with Mojang's signature.
#[derive(Clone, Debug)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

/// The account Mojang confirmed.
pub struct Profile {
    pub name: String,
    pub uuid: u128,
    pub properties: Vec<Property>,
}

struct Keys {
    private: RsaPrivateKey,
    /// Public key, DER (SubjectPublicKeyInfo), as the client expects it.
    public_der: Vec<u8>,
}

static KEYS: OnceLock<Keys> = OnceLock::new();

/// Generates the server's RSA key (1024 bits, like vanilla). Called once at startup when
/// online mode is on; takes a moment.
pub fn init() -> Result<()> {
    if KEYS.get().is_some() {
        return Ok(());
    }
    let private = RsaPrivateKey::new(&mut rand::rngs::OsRng, 1024).context("no se pudo generar la clave RSA")?;
    let public_der = private.to_public_key().to_public_key_der().context("clave pública")?.as_bytes().to_vec();
    let _ = KEYS.set(Keys { private, public_der });
    Ok(())
}

/// Writes a VarInt-prefixed byte array.
fn byte_array(p: &mut Vec<u8>, data: &[u8]) {
    p.varint(data.len() as i32);
    p.extend_from_slice(data);
}

fn read_byte_array<'a>(r: &mut Reader<'a>) -> Result<&'a [u8]> {
    let n = r.varint()?;
    ensure!((0..=1024).contains(&n), "arreglo de bytes inválido");
    r.bytes(n as usize)
}

/// Encrypts the connection and authenticates `name` with Mojang. `Ok(None)` means Mojang
/// does not know of this session (cracked client, or the account is not logged in).
pub async fn authenticate(conn: &mut Conn, name: &str) -> Result<Option<Profile>> {
    init()?;
    let keys = KEYS.get().unwrap();
    let token: [u8; 4] = rand::random();
    let mut p = packet(ENCRYPTION_REQUEST);
    p.string("");
    byte_array(&mut p, &keys.public_der);
    byte_array(&mut p, &token);
    conn.send(p);

    let body = tokio::time::timeout(TIMEOUT, conn.reader.next()).await.context("tiempo agotado")??.context("cerrada durante el cifrado")?;
    let mut r = Reader::new(&body);
    if r.varint()? != ENCRYPTION_RESPONSE {
        bail!("se esperaba encryption_response");
    }
    let secret = keys.private.decrypt(Pkcs1v15Encrypt, read_byte_array(&mut r)?).context("secreto compartido inválido")?;
    let echoed = keys.private.decrypt(Pkcs1v15Encrypt, read_byte_array(&mut r)?).context("token inválido")?;
    ensure!(echoed == token, "el token de verificación no coincide");
    let secret: [u8; 16] = secret.as_slice().try_into().context("el secreto compartido no mide 16 bytes")?;
    conn.reader.decrypt(secret);
    let _ = conn.tx.send(crate::net::Out::Encrypt(secret));

    let hash = server_hash(&secret, &keys.public_der);
    let path = format!("/session/minecraft/hasJoined?username={name}&serverId={hash}");
    let (status, body) = tokio::time::timeout(TIMEOUT, https_get(SESSION_HOST, &path)).await.context("la sesión de Mojang no respondió")??;
    match status {
        200 => {}
        204 | 403 => return Ok(None),
        s => bail!("la sesión de Mojang respondió {s}: {}", body.chars().take(200).collect::<String>()),
    }
    let json: serde_json::Value = serde_json::from_str(&body).context("respuesta de Mojang inválida")?;
    let uuid = json["id"].as_str().and_then(|s| u128::from_str_radix(s, 16).ok()).context("respuesta de Mojang sin id")?;
    let name = json["name"].as_str().unwrap_or(name).to_string();
    let properties = json["properties"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some(Property {
                        name: p["name"].as_str()?.to_string(),
                        value: p["value"].as_str()?.to_string(),
                        signature: p["signature"].as_str().map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    info!("{name} autenticado con Mojang ({})", super::format_uuid(uuid));
    Ok(Some(Profile { name, uuid, properties }))
}

/// The session hash: SHA-1 of (server id "", shared secret, public key) written as Java's
/// `BigInteger.toString(16)` (signed, two's complement, no leading zeros).
fn server_hash(secret: &[u8], public_der: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(secret);
    h.update(public_der);
    signed_hex(h.finalize().into())
}

fn signed_hex(mut digest: [u8; 20]) -> String {
    let negative = digest[0] & 0x80 != 0;
    if negative {
        // Two's complement: invert and add one.
        let mut carry = true;
        for b in digest.iter_mut().rev() {
            *b = !*b;
            if carry {
                let (v, c) = b.overflowing_add(1);
                *b = v;
                carry = c;
            }
        }
    }
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let hex = hex.trim_start_matches('0');
    format!("{}{}", if negative { "-" } else { "" }, if hex.is_empty() { "0" } else { hex })
}

/// A minimal HTTPS GET (HTTP/1.1, `Connection: close`). Returns the status and the body.
async fn https_get(host: &str, path: &str) -> Result<(u16, String)> {
    use tokio_rustls::rustls::{self, pki_types::ServerName, ClientConfig, RootCertStore};
    let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let mut config = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let tcp = tokio::net::TcpStream::connect((host, 443)).await?;
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(ServerName::try_from(host.to_string())?, tcp).await?;
    tls.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Bambietta\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let mut raw = Vec::new();
    // Some servers close without a TLS close_notify; what arrived is still good.
    if let Err(e) = tls.read_to_end(&mut raw).await {
        if raw.is_empty() {
            return Err(e.into());
        }
    }
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> Result<(u16, String)> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").context("respuesta HTTP incompleta")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).context("línea de estado HTTP inválida")?;
    let mut body = raw[split + 4..].to_vec();
    if head.lines().any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:") && l.to_ascii_lowercase().contains("chunked")) {
        body = dechunk(&body)?;
    }
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = data.windows(2).position(|w| w == b"\r\n").context("chunk incompleto")?;
        let size_text = String::from_utf8_lossy(&data[..eol]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16).context("tamaño de chunk inválido")?;
        data = &data[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        ensure!(data.len() >= size, "chunk truncado");
        out.extend_from_slice(&data[..size]);
        data = data.get(size + 2..).unwrap_or(&[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha1::{Digest, Sha1};

    fn digest(s: &str) -> String {
        signed_hex(Sha1::digest(s.as_bytes()).into())
    }

    #[test]
    fn minecraft_hex_digest() {
        // The examples from the protocol documentation.
        assert_eq!(digest("Notch"), "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48");
        assert_eq!(digest("jeb_"), "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1");
        assert_eq!(digest("simon"), "88e16a1019277b15d58faf0541e11910eb756f6");
    }

    #[test]
    fn cfb8_roundtrip() {
        let key = *b"0123456789abcdef";
        let mut data = b"Bambietta online mode".to_vec();
        let mut enc = crate::net::Cfb8::new(key);
        // In two pieces, as it would arrive from the network.
        let (a, b) = data.split_at_mut(5);
        enc.encrypt(a);
        enc.encrypt(b);
        assert_ne!(&data[..], b"Bambietta online mode");
        let mut dec = crate::net::Cfb8::new(key);
        dec.decrypt(&mut data);
        assert_eq!(&data[..], b"Bambietta online mode");
    }

    #[test]
    fn chunked_body() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).unwrap(), (200, "{\"a\":1}".to_string()));
    }
}
