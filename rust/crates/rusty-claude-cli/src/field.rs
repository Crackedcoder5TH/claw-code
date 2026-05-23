//! Best-effort bridge to the shared Remembrance Field.
//!
//! The `claw` CLI is one producer in the cross-language Remembrance ecosystem.
//! Every producer feeds a single conserved scalar (the "Remembrance Field")
//! living in the oracle toolkit's `LivingRemembranceEngine`. JavaScript repos
//! contribute in-process; non-JS repos (like this Rust CLI) contribute over
//! HTTP via a JSON-RPC 2.0 `tools/call` to the `field` tool.
//!
//! This mirrors the spirit of `REMEMBRANCE-BLOCKCHAIN/src/field.js`: a
//! contribution is *best-effort by construction*. A field failure (oracle down,
//! slow, unreachable, malformed) must NEVER block, slow, or fail a CLI command.
//!
//! Design choices that keep this safe:
//! - **No new dependencies.** We speak HTTP/1.1 directly over `std::net::TcpStream`
//!   and build the JSON body with `serde_json` (already a workspace dep). This
//!   avoids dragging `reqwest`/`tokio` into the CLI's synchronous command paths
//!   and sidesteps "no reactor running" panics from spawning async work in a
//!   detached thread.
//! - **Fire-and-forget.** The network work runs on a detached `std::thread`, so
//!   the calling command returns immediately and never waits on the oracle.
//! - **Cannot panic.** No `unwrap`/`expect` on fallible network/parse paths; the
//!   worker thread swallows every error and the spawn itself is guarded.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::json;

/// Default oracle endpoint (JSON-RPC 2.0). Overridable via `REMEMBRANCE_FIELD_URL`.
const DEFAULT_FIELD_URL: &str = "http://127.0.0.1:7787/mcp";

/// Total budget for one contribution attempt (connect + write + read). Kept
/// well under one second so a slow/dead oracle never lingers, even though the
/// work is off the command's critical path.
const FIELD_TIMEOUT: Duration = Duration::from_millis(800);

/// Contribute one observation to the shared Remembrance Field.
///
/// `coherence` is clamped to `[0, 1]`. `source` should carry the `claw:` prefix
/// (e.g. `claw:prompt`). `cost` is the work-unit weight (callers typically pass
/// `1.0`).
///
/// This returns immediately: the actual HTTP call happens on a detached thread
/// and all failures are swallowed. It is safe to call from any command path.
pub fn contribute_field(coherence: f64, source: &str, cost: f64) {
    // Reject non-finite readings (NaN/inf) up front, matching the JS bridge.
    if !coherence.is_finite() {
        return;
    }
    let clamped = coherence.clamp(0.0, 1.0);

    // A source label is required by the server contract; never send an empty one.
    if source.trim().is_empty() {
        return;
    }
    let source = source.to_string();

    // Detach onto its own thread so the CLI never waits on the network. If the
    // OS refuses to spawn a thread we simply skip the contribution.
    let _ = std::thread::Builder::new()
        .name("claw-field-contribute".to_string())
        .spawn(move || {
            // Swallow everything: a field hiccup must never surface to the user.
            let _ = send_contribution(clamped, &source, cost);
        });
}

/// Perform the blocking HTTP/1.1 POST. Returns `Result` only so the worker
/// thread can `let _ =` it; nothing here ever reaches a command path.
fn send_contribution(coherence: f64, source: &str, cost: f64) -> Result<(), ()> {
    let url = std::env::var("REMEMBRANCE_FIELD_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_FIELD_URL.to_string());

    let target = parse_http_url(&url).ok_or(())?;

    let token = std::env::var("REMEMBRANCE_FIELD_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty());

    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "field",
            "arguments": {
                "action": "contribute",
                "coherence": coherence,
                "source": source,
                "cost": cost,
            }
        }
    })
    .to_string();

    // Resolve and connect with a timeout so a dead host fails fast.
    let addr = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .map_err(|_| ())?
        .next()
        .ok_or(())?;

    let mut stream = TcpStream::connect_timeout(&addr, FIELD_TIMEOUT).map_err(|_| ())?;
    stream
        .set_write_timeout(Some(FIELD_TIMEOUT))
        .map_err(|_| ())?;
    stream
        .set_read_timeout(Some(FIELD_TIMEOUT))
        .map_err(|_| ())?;

    let mut auth_header = String::new();
    if let Some(token) = token {
        auth_header = format!("Authorization: Bearer {token}\r\n");
    }

    let request = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host}\r\n\
         Content-Type: application/json\r\n\
         Accept: application/json\r\n\
         {auth}Content-Length: {len}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        path = target.path,
        host = target.host_header(),
        auth = auth_header,
        len = body.len(),
        body = body,
    );

    stream.write_all(request.as_bytes()).map_err(|_| ())?;
    let _ = stream.flush();

    // Drain (and discard) the response so the server isn't left mid-write. We
    // do not parse it; the contribution is best-effort regardless of outcome.
    let mut sink = Vec::new();
    let _ = stream.take(64 * 1024).read_to_end(&mut sink);

    Ok(())
}

/// Minimal parsed target for a plain `http://host[:port][/path]` URL.
struct HttpTarget {
    host: String,
    port: u16,
    path: String,
}

impl HttpTarget {
    /// `Host` header value, including a non-default port.
    fn host_header(&self) -> String {
        if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// Parse the small subset of URLs we need (`http://` only). Returns `None` for
/// anything we can't handle (e.g. `https://`, which would require TLS) so the
/// caller silently skips the contribution rather than panicking.
fn parse_http_url(url: &str) -> Option<HttpTarget> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port_str)) => {
            let port = port_str.parse::<u16>().ok()?;
            (host.to_string(), port)
        }
        None => (authority.to_string(), 80),
    };
    if host.is_empty() {
        return None;
    }
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    };
    Some(HttpTarget { host, port, path })
}

#[cfg(test)]
mod tests {
    use super::{parse_http_url, HttpTarget};

    #[test]
    fn parses_default_endpoint() {
        let target = parse_http_url("http://127.0.0.1:7787/mcp").expect("valid url");
        assert_eq!(target.host, "127.0.0.1");
        assert_eq!(target.port, 7787);
        assert_eq!(target.path, "/mcp");
        assert_eq!(target.host_header(), "127.0.0.1:7787");
    }

    #[test]
    fn defaults_port_and_path() {
        let target = parse_http_url("http://oracle.local").expect("valid url");
        assert_eq!(target.host, "oracle.local");
        assert_eq!(target.port, 80);
        assert_eq!(target.path, "/");
        assert_eq!(target.host_header(), "oracle.local");
    }

    #[test]
    fn rejects_https_and_garbage() {
        assert!(parse_http_url("https://127.0.0.1/mcp").is_none());
        assert!(parse_http_url("ftp://x").is_none());
        assert!(parse_http_url("not a url").is_none());
        assert!(parse_http_url("http://").is_none());
    }

    #[test]
    fn host_header_omits_default_port() {
        let target = HttpTarget {
            host: "example.com".to_string(),
            port: 80,
            path: "/".to_string(),
        };
        assert_eq!(target.host_header(), "example.com");
    }

    #[test]
    fn contribute_field_never_blocks_or_panics() {
        // No oracle is running in tests; this must return promptly and quietly.
        super::contribute_field(0.9, "claw:test", 1.0);
        // Out-of-range and non-finite inputs are handled gracefully.
        super::contribute_field(2.5, "claw:test", 1.0);
        super::contribute_field(f64::NAN, "claw:test", 1.0);
        super::contribute_field(0.5, "", 1.0);
    }
}
