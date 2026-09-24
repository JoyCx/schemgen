//! Who may call the API.
//!
//! The server belongs to the machine it runs on, and three checks keep it
//! there:
//!
//! * **Host** — a request must be addressed to `localhost`, `127.0.0.1` or
//!   `[::1]` (or a name allowed with `--allow-host`). A DNS-rebinding page
//!   reaches 127.0.0.1 under its own hostname, and that name is in `Host`.
//! * **Origin** — browsers send `Origin` on cross-origin requests. One from a
//!   page that is not on this machine is refused, so a website cannot post a
//!   conversion (a multipart POST needs no CORS preflight) or open a file
//!   manager.
//! * **Token** — with `--token`, every `/api` call except `/api/health` must
//!   carry `Authorization: Bearer <token>`, or `?token=` where a header cannot
//!   be set (`EventSource`, `<img>`, download links). Any local process can
//!   reach a loopback port; only the one that launched the server knows the
//!   token.

use actix_web::body::{EitherBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::{header, StatusCode};
use actix_web::middleware::Next;
use actix_web::{web, Error, HttpResponse};

use crate::state::AppState;

/// Names every server accepts, whatever else it was told.
pub const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]", "::1"];

pub async fn guard<B: MessageBody>(
    req: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<EitherBody<B>>, Error> {
    let Some(app) = req
        .app_data::<web::Data<std::sync::Arc<AppState>>>()
        .cloned()
    else {
        return next.call(req).await.map(|r| r.map_into_left_body());
    };

    if let Some(host) = header_str(&req, header::HOST) {
        if !is_allowed(host_name(host), &app.allowed_hosts) {
            return Ok(reject(req, StatusCode::MISDIRECTED_REQUEST, "Unknown host"));
        }
    }
    if let Some(origin) = header_str(&req, header::ORIGIN) {
        let allowed = origin_host(origin).is_some_and(|h| is_allowed(h, &app.allowed_hosts));
        if !allowed {
            return Ok(reject(
                req,
                StatusCode::FORBIDDEN,
                "Cross-origin requests are not allowed",
            ));
        }
    }

    let path = req.path();
    if let Some(expected) = app.token.as_deref() {
        if path.starts_with("/api/") && path != "/api/health" && !carries_token(&req, expected) {
            return Ok(reject(
                req,
                StatusCode::UNAUTHORIZED,
                "Missing or wrong token — send Authorization: Bearer <token>",
            ));
        }
    }

    next.call(req).await.map(|r| r.map_into_left_body())
}

fn reject<B>(
    req: ServiceRequest,
    status: StatusCode,
    message: &str,
) -> ServiceResponse<EitherBody<B>> {
    let response = HttpResponse::build(status).json(serde_json::json!({ "error": message }));
    req.into_response(response).map_into_right_body()
}

fn header_str(req: &ServiceRequest, name: header::HeaderName) -> Option<&str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

fn carries_token(req: &ServiceRequest, expected: &str) -> bool {
    let bearer = header_str(req, header::AUTHORIZATION)
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    if bearer.is_some_and(|t| constant_time_eq(t, expected)) {
        return true;
    }
    web::Query::<std::collections::HashMap<String, String>>::from_query(req.query_string())
        .ok()
        .and_then(|q| q.get("token").map(|t| constant_time_eq(t, expected)))
        .unwrap_or(false)
}

/// Compare without stopping at the first difference, so response timing
/// says nothing about how much of a guess was right.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `host[:port]` → `host`, keeping IPv6 brackets: `[::1]:3001` → `[::1]`.
pub fn host_name(value: &str) -> &str {
    let value = value.trim();
    if value.starts_with('[') {
        match value.find(']') {
            Some(end) => &value[..=end],
            None => value,
        }
    } else {
        value.split(':').next().unwrap_or(value)
    }
}

/// The host of an `Origin` header (`scheme://host[:port]`); `None` for
/// `null` and anything else that is not a URL origin.
fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin.trim().split_once("://")?.1;
    let authority = rest.split('/').next().unwrap_or(rest);
    Some(host_name(authority))
}

fn is_allowed(host: &str, extra: &[String]) -> bool {
    LOOPBACK_HOSTS.iter().any(|h| h.eq_ignore_ascii_case(host))
        || extra.iter().any(|h| h.eq_ignore_ascii_case(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_names_drop_the_port() {
        assert_eq!(host_name("localhost:3001"), "localhost");
        assert_eq!(host_name("127.0.0.1"), "127.0.0.1");
        assert_eq!(host_name("[::1]:3001"), "[::1]");
        assert_eq!(host_name("evil.example"), "evil.example");
    }

    #[test]
    fn origins_parse_to_hosts() {
        assert_eq!(origin_host("http://localhost:5173"), Some("localhost"));
        assert_eq!(origin_host("https://evil.example"), Some("evil.example"));
        assert_eq!(origin_host("null"), None);
    }

    #[test]
    fn only_loopback_and_listed_hosts_pass() {
        assert!(is_allowed("LOCALHOST", &[]));
        assert!(is_allowed("[::1]", &[]));
        assert!(!is_allowed("rebind.attacker.example", &[]));
        assert!(is_allowed("my-pc.lan", &["my-pc.lan".to_string()]));
    }

    #[test]
    fn token_comparison() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}
