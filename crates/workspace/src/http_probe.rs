//! Bounded GET against an already running, registered project site only.
//! Never resolves client-provided authorities, uses a proxy, follows redirects,
//! returns response bodies or exposes response headers.
use crate::{Result, Workspace, error};
use serde::Serialize;
use std::{
    net::{Ipv4Addr, SocketAddr},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize)]
pub struct HttpProbeResult {
    pub status: u16,
    pub elapsed_ms: u64,
    pub context_id: String,
    pub tls: bool,
    pub redirect: bool,
}

/// A manager-internal snapshot of one running, registered site. Clients never
/// supply or receive its network authority or trust material.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpProbeTarget {
    host: String,
    session_nonce: String,
    port: u16,
    tls: bool,
    ca: Option<Vec<u8>>,
}
impl Workspace {
    pub fn http_probe_target(&self, project: &str) -> Result<HttpProbeTarget> {
        let session = self
            .sessions
            .get(project)
            .ok_or_else(|| error("precondition_failed"))?;
        let status = session.web.status();
        if !status.running {
            return Err(error("precondition_failed"));
        }
        let site = status
            .sites
            .iter()
            .find(|site| site.project_id == project && site.php_ready)
            .ok_or_else(|| error("precondition_failed"))?;
        devforge_serving::site::domain(&site.domain).map_err(|_| error("precondition_failed"))?;
        let tls = site.https_url.is_some();
        if tls && !site.certificate_active {
            return Err(error("precondition_failed"));
        }
        let port = if tls {
            status.https.selected
        } else {
            status.http.selected
        };
        let ca = if tls {
            let path = self.sites.root().join("authority").join("ca.der");
            let metadata =
                std::fs::symlink_metadata(&path).map_err(|_| error("precondition_failed"))?;
            if !metadata.is_file() || metadata.len() > 65536 {
                return Err(error("precondition_failed"));
            }
            let ca = std::fs::read(path).map_err(|_| error("precondition_failed"))?;
            devforge_serving::authority::validate_root(
                &ca,
                &self
                    .sites
                    .owner()
                    .map_err(|_| error("precondition_failed"))?,
            )
            .map_err(|_| error("precondition_failed"))?;
            Some(ca)
        } else {
            None
        };
        Ok(HttpProbeTarget {
            host: site.domain.clone(),
            session_nonce: status.nonce,
            port,
            tls,
            ca,
        })
    }
}
impl HttpProbeTarget {
    /// Blocking network IO must run outside the workspace actor. The caller
    /// rechecks project authority and target freshness before and after this call.
    pub fn probe(
        &self,
        path: &str,
        context_id: &str,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<HttpProbeResult> {
        probe(
            &self.host,
            self.port,
            self.tls,
            self.ca.as_deref(),
            path,
            context_id,
            check,
        )
    }
}

fn probe(
    host: &str,
    port: u16,
    tls: bool,
    ca: Option<&[u8]>,
    path: &str,
    context_id: &str,
    check: &dyn Fn() -> Result<()>,
) -> Result<HttpProbeResult> {
    if port == 0
        || path.is_empty()
        || path.len() > 1024
        || !path.starts_with('/')
        || path.starts_with("//")
        || path
            .bytes()
            .any(|b| b <= 0x20 || b >= 0x7f || matches!(b, b'?' | b'#' | b'\\'))
        || context_id.len() != 32
        || !context_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(error("invalid_input"));
    }
    let base = format!("{}://{}:{}", if tls { "https" } else { "http" }, host, port);
    let url = reqwest::Url::parse(&format!("{base}{path}")).map_err(|_| error("invalid_input"))?;
    if url.host_str() != Some(host)
        || url.port_or_known_default() != Some(port)
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(error("invalid_input"));
    }
    let mut builder = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(3))
        .resolve(host, SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    if tls {
        let root = reqwest::Certificate::from_der(ca.ok_or_else(|| error("precondition_failed"))?)
            .map_err(|_| error("precondition_failed"))?;
        builder = builder.tls_certs_only(vec![root]);
    }
    let client = builder.build().map_err(|_| error("storage_unavailable"))?;
    check()?;
    let start = Instant::now();
    let response = client
        .get(url)
        .header("X-DevForge-Context", context_id)
        .send()
        .map_err(|err| {
            error(if err.is_timeout() {
                "timed_out"
            } else {
                "probe_failed"
            })
        })?;
    let elapsed_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let status = response.status();
    check()?;
    Ok(HttpProbeResult {
        status: status.as_u16(),
        elapsed_ms,
        context_id: context_id.into(),
        tls,
        redirect: status.is_redirection(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::Arc,
        thread,
    };
    const CONTEXT: &str = "abcdef0123456789abcdef0123456789";
    fn allow() -> Result<()> {
        Ok(())
    }
    fn listener() -> TcpListener {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap()
    }
    #[test]
    fn local_target_pins_loopback_and_never_follows_redirect_or_exposes_response() {
        let source = listener();
        let port = source.local_addr().unwrap().port();
        let outside = listener();
        outside.set_nonblocking(true).unwrap();
        let dest = outside.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut conn, _) = source.accept().unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut request = [0; 2048];
            let n = conn.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..n]).to_ascii_lowercase();
            assert!(request.contains("host: example.devforge.test:"));
            assert!(request.contains(&format!("x-devforge-context: {CONTEXT}")));
            write!(conn, "HTTP/1.1 302 Found\r\nLocation: http://{dest}/private\r\nContent-Length: 16\r\nConnection: close\r\n\r\nprivate-canary!!").unwrap();
        });
        let result = probe(
            "example.devforge.test",
            port,
            false,
            None,
            "/health",
            CONTEXT,
            &allow,
        )
        .unwrap();
        assert_eq!(result.status, 302);
        assert!(result.redirect);
        assert!(!result.tls);
        assert_eq!(result.context_id, CONTEXT);
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(!serialized.contains("private"));
        worker.join().unwrap();
        assert!(outside.accept().is_err());
    }
    #[test]
    fn rejects_authorities_queries_and_invalid_context_before_network() {
        for path in [
            "https://attacker/",
            "//attacker/",
            "/x?token=secret",
            "/x#fragment",
            "/x\\@attacker",
            "/x\r\nInjected: y",
        ] {
            assert!(
                probe(
                    "example.devforge.test",
                    1,
                    false,
                    None,
                    path,
                    CONTEXT,
                    &allow
                )
                .is_err(),
                "{path:?}"
            );
        }
        assert!(probe("example.devforge.test", 1, false, None, "/", "ABC", &allow).is_err());
        assert!(probe("example.devforge.test", 1, true, None, "/", CONTEXT, &allow).is_err());
    }
    #[test]
    fn deadline_refusal_before_send_does_not_connect() {
        let server = listener();
        server.set_nonblocking(true).unwrap();
        let port = server.local_addr().unwrap().port();
        let check = || Err(error("timed_out"));
        let failure = probe(
            "example.devforge.test",
            port,
            false,
            None,
            "/",
            CONTEXT,
            &check,
        )
        .unwrap_err();
        assert_eq!(failure.code, "timed_out");
        assert!(server.accept().is_err());
    }
    #[test]
    fn authorization_is_rechecked_after_response_before_return() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let server = listener();
        let port = server.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0; 2048];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
        });
        let calls = AtomicUsize::new(0);
        let check = || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(error("revoked"))
            }
        };
        let failure = probe(
            "example.devforge.test",
            port,
            false,
            None,
            "/",
            CONTEXT,
            &check,
        )
        .unwrap_err();
        assert_eq!(failure.code, "revoked");
        worker.join().unwrap();
    }
    #[test]
    fn stalled_response_is_bounded() {
        let server = listener();
        let port = server.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (_stream, _) = server.accept().unwrap();
            thread::sleep(Duration::from_millis(3300));
        });
        let start = Instant::now();
        let timed_out = probe(
            "example.devforge.test",
            port,
            false,
            None,
            "/",
            CONTEXT,
            &allow,
        )
        .unwrap_err();
        assert_eq!(timed_out.code, "timed_out");
        assert!(start.elapsed() < Duration::from_secs(8));
        worker.join().unwrap();
    }
    #[test]
    fn tls_requires_correct_managed_trust_and_hostname() {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["example.devforge.test".into()]).unwrap();
        let cert_der = cert.der().clone();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key.into())
            .unwrap();
        let config = Arc::new(config);
        let server = listener();
        let port = server.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            for _ in 0..3 {
                let (stream, _) = server.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let conn = rustls::ServerConnection::new(config.clone()).unwrap();
                let mut tls = rustls::StreamOwned::new(conn, stream);
                let mut buf = [0; 2048];
                if tls.read(&mut buf).is_ok() {
                    let _ = tls.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
            }
        });
        let wrong =
            rcgen::generate_simple_self_signed(vec!["example.devforge.test".into()]).unwrap();
        assert!(
            probe(
                "example.devforge.test",
                port,
                true,
                Some(wrong.cert.der().as_ref()),
                "/",
                CONTEXT,
                &allow
            )
            .is_err()
        );
        assert!(
            probe(
                "wrong.devforge.test",
                port,
                true,
                Some(cert_der.as_ref()),
                "/",
                CONTEXT,
                &allow
            )
            .is_err()
        );
        let result = probe(
            "example.devforge.test",
            port,
            true,
            Some(cert_der.as_ref()),
            "/",
            CONTEXT,
            &allow,
        )
        .unwrap();
        assert_eq!(result.status, 200);
        assert!(result.tls);
        worker.join().unwrap();
    }
}

#[cfg(test)]
#[path = "http_probe_e2e.rs"]
mod e2e;
