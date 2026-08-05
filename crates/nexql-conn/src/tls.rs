//! Shared rustls connector for pooled and one-shot Postgres connections.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::str::FromStr;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};
use tokio_postgres::Config;
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::error::ConnError;
use crate::resolve::ConnectionParams;

/// Whether the libpq sslmode string requires TLS (not NoTls).
pub fn sslmode_needs_tls(sslmode: Option<&str>) -> bool {
    matches!(
        sslmode.map(str::to_ascii_lowercase).as_deref(),
        Some("require") | Some("verify-ca") | Some("verify-full")
    )
}

/// Whether resolved params require a TLS connector.
pub fn connection_needs_tls(params: &ConnectionParams) -> bool {
    sslmode_needs_tls(params.sslmode.as_deref())
}

/// Parse connection URL into a tokio-postgres config (shared by pool + connect_once).
///
/// tokio-postgres only understands `disable` / `prefer` / `require`; map verify-* to
/// `require` so Config parsing succeeds while rustls still verifies CAs.
pub fn pg_config_from_params(params: &ConnectionParams) -> Result<Config, ConnError> {
    let mut url = params.to_url()?;
    if let Some(mode) = params.sslmode.as_deref() {
        match mode.to_ascii_lowercase().as_str() {
            "verify-ca" | "verify-full" => {
                url = url
                    .replace("sslmode=verify-ca", "sslmode=require")
                    .replace("sslmode=verify-full", "sslmode=require");
            }
            _ => {}
        }
    }
    Config::from_str(&url).map_err(|e| ConnError::Pool(e.to_string()))
}

/// Build a rustls connector for TLS sslmodes (`require`, `verify-ca`, `verify-full`).
pub fn build_rustls_connector(params: &ConnectionParams) -> Result<MakeRustlsConnect, ConnError> {
    if !connection_needs_tls(params) {
        return Err(ConnError::Pool(
            "build_rustls_connector called for non-TLS sslmode".into(),
        ));
    }

    let is_require_only = matches!(
        params.sslmode.as_deref().map(str::to_ascii_lowercase).as_deref(),
        Some("require")
    ) && params.sslrootcert.is_none();

    let client_auth = if let (Some(cert_path), Some(key_path)) =
        (params.sslcert.as_deref(), params.sslkey.as_deref())
    {
        let certs = load_certs(Path::new(cert_path))?;
        let key = load_private_key(Path::new(key_path))?;
        Some((certs, key))
    } else if params.sslcert.is_some() || params.sslkey.is_some() {
        return Err(ConnError::Config(
            "sslcert and sslkey must both be set for client certificate auth".into(),
        ));
    } else {
        None
    };

    let tls_config = if is_require_only {
        let builder = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(NoServerCertVerifier));
        if let Some((certs, key)) = client_auth {
            builder
                .with_client_auth_cert(certs, key)
                .map_err(|e| ConnError::Config(format!("client auth TLS config: {e}")))?
        } else {
            builder.with_no_client_auth()
        }
    } else {
        let root_store = load_root_store(params)?;
        let builder = ClientConfig::builder().with_root_certificates(root_store);
        if let Some((certs, key)) = client_auth {
            builder
                .with_client_auth_cert(certs, key)
                .map_err(|e| ConnError::Config(format!("client auth TLS config: {e}")))?
        } else {
            builder.with_no_client_auth()
        }
    };

    Ok(MakeRustlsConnect::new(tls_config))
}

#[derive(Debug)]
struct NoServerCertVerifier;

impl rustls::client::danger::ServerCertVerifier for NoServerCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
        ]
    }
}

fn load_root_store(params: &ConnectionParams) -> Result<RootCertStore, ConnError> {
    let mut root_store = RootCertStore::empty();
    if let Some(root_path) = params.sslrootcert.as_deref() {
        add_pem_roots(Path::new(root_path), &mut root_store)?;
        if root_store.is_empty() {
            return Err(ConnError::Config(format!(
                "sslrootcert {root_path} contained no valid CA certificates"
            )));
        }
        return Ok(root_store);
    }

    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        let _ = root_store.add(cert);
    }
    if root_store.is_empty() {
        return Err(ConnError::Config(
            "no native root certificates available for TLS".into(),
        ));
    }
    Ok(root_store)
}

fn add_pem_roots(path: &Path, store: &mut RootCertStore) -> Result<(), ConnError> {
    let file = File::open(path)
        .map_err(|e| ConnError::Config(format!("sslrootcert {}: {e}", path.display())))?;
    let mut reader = BufReader::new(file);
    for item in rustls_pemfile::certs(&mut reader) {
        let der = item.map_err(|e| ConnError::Config(format!("sslrootcert PEM: {e}")))?;
        store
            .add(der)
            .map_err(|e| ConnError::Config(format!("sslrootcert CA: {e}")))?;
    }
    Ok(())
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, ConnError> {
    let file = File::open(path)
        .map_err(|e| ConnError::Config(format!("sslcert {}: {e}", path.display())))?;
    let mut reader = BufReader::new(file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ConnError::Config(format!("sslcert PEM: {e}")))?;
    if certs.is_empty() {
        return Err(ConnError::Config(format!(
            "sslcert {} contained no certificates",
            path.display()
        )));
    }
    Ok(certs)
}

fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, ConnError> {
    let file = File::open(path)
        .map_err(|e| ConnError::Config(format!("sslkey {}: {e}", path.display())))?;
    let mut reader = BufReader::new(file);
    let key = rustls_pemfile::private_key(&mut reader)
        .map_err(|e| ConnError::Config(format!("sslkey PEM: {e}")))?
        .ok_or_else(|| {
            ConnError::Config(format!(
                "sslkey {} contained no private key",
                path.display()
            ))
        })?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::params_from_url;

    #[test]
    fn sslmode_needs_tls_mapping() {
        assert!(!sslmode_needs_tls(Some("disable")));
        assert!(!sslmode_needs_tls(Some("prefer")));
        assert!(sslmode_needs_tls(Some("require")));
        assert!(sslmode_needs_tls(Some("verify-ca")));
        assert!(sslmode_needs_tls(Some("verify-full")));
    }

    #[test]
    fn build_rustls_connector_accepts_require_without_custom_ca() {
        let params = params_from_url("postgres://u@h:5432/db?sslmode=require").unwrap();
        assert!(build_rustls_connector(&params).is_ok());
    }

    #[test]
    fn build_rustls_connector_accepts_verify_full_with_native_roots() {
        let params = params_from_url("postgres://u@h:5432/db?sslmode=verify-full").unwrap();
        assert!(build_rustls_connector(&params).is_ok());
    }

    #[test]
    fn pg_config_from_params_accepts_verify_full() {
        let params = params_from_url("postgres://u@h:5432/db?sslmode=verify-full").unwrap();
        assert!(pg_config_from_params(&params).is_ok());
    }
}
