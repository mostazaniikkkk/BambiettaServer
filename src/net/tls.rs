//! TLS for `wss://`: own certificate (PEM) or a self-signed one generated for development.

use crate::config::Config;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::{self, ServerConfig};
use tokio_rustls::TlsAcceptor;
use tracing::{info, warn};

/// Builds the TLS acceptor from the configuration (or `None` if TLS is disabled).
pub fn acceptor(cfg: &Config) -> Result<Option<TlsAcceptor>> {
    let (cert_path, key_path) = match (&cfg.tls_cert, &cfg.tls_key) {
        (Some(c), Some(k)) => (PathBuf::from(c), PathBuf::from(k)),
        (None, None) if cfg.tls_self_signed => self_signed(cfg)?,
        (None, None) => return Ok(None),
        _ => bail!("tls-cert y tls-key tienen que ir juntos"),
    };
    let certs = load_certs(&cert_path)?;
    let key = load_key(&key_path)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("certificado o clave TLS inválidos")?;
    info!("TLS activo ({}): se aceptan conexiones wss://", cert_path.display());
    Ok(Some(TlsAcceptor::from(Arc::new(config))))
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let data = std::fs::read(path).with_context(|| format!("no se pudo leer {}", path.display()))?;
    let certs = rustls_pemfile::certs(&mut data.as_slice()).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        bail!("{} no contiene certificados", path.display());
    }
    Ok(certs)
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let data = std::fs::read(path).with_context(|| format!("no se pudo leer {}", path.display()))?;
    rustls_pemfile::private_key(&mut data.as_slice())?.with_context(|| format!("{} no contiene una clave privada", path.display()))
}

/// Generates (only once) a self-signed certificate in `tls/`. Development only:
/// the browser will ask to accept it by visiting `https://host:port` before using `wss://`.
fn self_signed(cfg: &Config) -> Result<(PathBuf, PathBuf)> {
    let dir = PathBuf::from("tls");
    let (cert, key) = (dir.join("selfsigned-cert.pem"), dir.join("selfsigned-key.pem"));
    if cert.exists() && key.exists() {
        return Ok((cert, key));
    }
    std::fs::create_dir_all(&dir)?;
    let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    names.extend(cfg.tls_hostnames.iter().cloned());
    if cfg.host != "0.0.0.0" && !names.contains(&cfg.host) {
        names.push(cfg.host.clone());
    }
    let generated = rcgen::generate_simple_self_signed(names.clone())?;
    std::fs::write(&cert, generated.cert.pem())?;
    std::fs::write(&key, generated.key_pair.serialize_pem())?;
    warn!("certificado autofirmado generado para {names:?}; en producción usa uno real (tls-cert/tls-key)");
    Ok((cert, key))
}
