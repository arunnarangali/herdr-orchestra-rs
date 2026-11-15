//! Real requests over a real loopback socket against the real router.

use axum::routing::get;
use orch_api::doc;
use orch_api::{Api, ApiError, AppState, Token, build_with};
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct Resp {
    status: u16,
    headers: String,
    body: String,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }
    fn header(&self, name: &str) -> Option<String> {
        self.headers.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    }
}

struct Server {
    addr: std::net::SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

fn extra(api: orch_api::ApiRouter) -> orch_api::ApiRouter {
    async fn boom() -> &'static str {
        panic!("secret detail: /home/me/.config/token");
    }
    async fn broken() -> Result<&'static str, ApiError> {
        Err(ApiError::internal("db exploded at /srv/state.db"))
    }
    async fn needs_run(
        axum::extract::State(s): axum::extract::State<AppState>,
    ) -> Result<&'static str, ApiError> {
        s.open_store().map(|_| "ok")
    }
    async fn ping() -> &'static str {
        "pong"
    }
    api.route(
        "/test/boom",
        get(boom),
        doc::get("boom", "panics", "ErrorBody"),
    )
    .route(
        "/test/broken",
        get(broken),
        doc::get("broken", "fails", "ErrorBody"),
    )
    .route(
        "/test/run",
        get(needs_run),
        doc::get("run", "needs a run", "ErrorBody"),
    )
    .route(
        "/stream/ping",
        get(ping),
        doc::get("ping", "stream-like", "ErrorBody"),
    )
}

async fn start(root: &Path) -> Server {
    let state = AppState::new(root.to_path_buf(), Token::new(SECRET));
    let listener = orch_api::bind("127.0.0.1", 0).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    // The server under test is `orch_api::serve`; `extra` routes need `build_with`, so serve
    // the same router the same way.
    let Api { router, .. } = build_with(state.clone(), extra);
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await
    });
    Server {
        addr,
        stop: Some(tx),
        task,
    }
}

async fn request(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> Resp {
    let mut req = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        req.push_str(&format!("Host: {addr}\r\n"));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let raw = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let (status_line, headers) = head.split_once("\r\n").unwrap_or((head, ""));
    let status = status_line.split(' ').nth(1).unwrap().parse().unwrap();
    // Bodies may be chunked; the JSON here is small, so strip chunk framing if present.
    let body = if headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        let mut out = String::new();
        let mut rest = body;
        while let Some((len, tail)) = rest.split_once("\r\n") {
            let n = usize::from_str_radix(len.trim(), 16).unwrap_or(0);
            if n == 0 {
                break;
            }
            out.push_str(&tail[..n]);
            rest = &tail[n + 2..];
        }
        out
    } else {
        body.to_string()
    };
    Resp {
        status,
        headers: headers.to_string(),
        body,
    }
}

async fn get_auth(addr: std::net::SocketAddr, path: &str) -> Resp {
    request(
        addr,
        "GET",
        path,
        &[("Authorization", &format!("Bearer {SECRET}"))],
    )
    .await
}

fn assert_error(r: &Resp, status: u16, code: &str) {
    assert_eq!(r.status, status, "{}", r.body);
    assert!(
        r.header("content-type")
            .unwrap()
            .starts_with("application/json")
    );
    let j = r.json();
    let e = &j["error"];
    assert_eq!(e["code"], code, "{j}");
    assert!(e["message"].as_str().is_some_and(|m| !m.is_empty()));
    let id = e["request_id"].as_str().unwrap();
    assert!(!id.is_empty() && id != "-");
    assert_eq!(r.header("x-request-id").as_deref(), Some(id));
    assert_eq!(j.as_object().unwrap().len(), 1);
    assert_eq!(e.as_object().unwrap().len(), 3);
    assert!(!r.body.contains(SECRET));
}

#[tokio::test]
async fn health_needs_the_token_and_reports_version_and_project() {
    let dir = tempfile::tempdir().unwrap();
    let srv = start(dir.path()).await;

    let r = get_auth(srv.addr, "/api/v1/health").await;
    assert_eq!(r.status, 200);
    let j = r.json();
    assert_eq!(j["status"], "ok");
    assert_eq!(j["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(j["project"], dir.path().display().to_string());
    assert_eq!(j["run"], false);

    let none = request(srv.addr, "GET", "/api/v1/health", &[]).await;
    assert_error(&none, 401, "unauthorized");
    assert_eq!(none.header("www-authenticate").as_deref(), Some("Bearer"));
    let wrong = request(
        srv.addr,
        "GET",
        "/api/v1/health",
        &[("Authorization", "Bearer nope")],
    )
    .await;
    assert_error(&wrong, 401, "unauthorized");
    let basic = request(
        srv.addr,
        "GET",
        "/api/v1/health",
        &[("Authorization", &format!("Basic {SECRET}"))],
    )
    .await;
    assert_error(&basic, 401, "unauthorized");
    // The token in the query string is not accepted outside the stream routes.
    let q = request(
        srv.addr,
        "GET",
        &format!("/api/v1/health?token={SECRET}"),
        &[],
    )
    .await;
    assert_error(&q, 401, "unauthorized");
}

#[tokio::test]
async fn stream_routes_also_take_the_token_as_a_query_parameter() {
    let dir = tempfile::tempdir().unwrap();
    let srv = start(dir.path()).await;
    let ok = request(
        srv.addr,
        "GET",
        &format!("/api/v1/stream/ping?token={SECRET}"),
        &[],
    )
    .await;
    assert_eq!((ok.status, ok.body.as_str()), (200, "pong"));
    let bad = request(srv.addr, "GET", "/api/v1/stream/ping?token=wrong", &[]).await;
    assert_error(&bad, 401, "unauthorized");
    // A look-alike prefix is not the stream.
    let near = request(
        srv.addr,
        "GET",
        &format!("/api/v1/streamer?token={SECRET}"),
        &[],
    )
    .await;
    assert_error(&near, 401, "unauthorized");
}

#[tokio::test]
async fn unknown_routes_and_wrong_methods_use_the_error_body() {
    let dir = tempfile::tempdir().unwrap();
    let srv = start(dir.path()).await;
    assert_error(
        &get_auth(srv.addr, "/api/v1/nothing-here").await,
        404,
        "not_found",
    );
    assert_error(&get_auth(srv.addr, "/").await, 404, "not_found");
    // Unknown routes do not reveal themselves to a caller without the token.
    assert_error(
        &request(srv.addr, "GET", "/api/v1/nothing-here", &[]).await,
        401,
        "unauthorized",
    );
    let post = request(
        srv.addr,
        "POST",
        "/api/v1/health",
        &[
            ("Authorization", &format!("Bearer {SECRET}")),
            ("Content-Length", "0"),
        ],
    )
    .await;
    assert_error(&post, 405, "method_not_allowed");
}

#[tokio::test]
async fn panics_and_internal_errors_are_500_without_internals() {
    let dir = tempfile::tempdir().unwrap();
    let srv = start(dir.path()).await;
    for path in ["/api/v1/test/boom", "/api/v1/test/broken"] {
        let r = get_auth(srv.addr, path).await;
        assert_error(&r, 500, "internal");
        assert_eq!(r.json()["error"]["message"], "internal error");
        assert!(!r.body.contains("secret detail") && !r.body.contains("/srv/state.db"));
    }
    // The server is still up after a panic.
    assert_eq!(get_auth(srv.addr, "/api/v1/health").await.status, 200);
}

#[tokio::test]
async fn no_run_is_404_and_nothing_is_created() {
    let dir = tempfile::tempdir().unwrap();
    let srv = start(dir.path()).await;
    let r = get_auth(srv.addr, "/api/v1/test/run").await;
    assert_error(&r, 404, "no_run");
    assert!(
        r.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no run")
    );
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "the server created something"
    );
}

#[tokio::test]
async fn cross_origin_requests_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let srv = start(dir.path()).await;
    let auth = format!("Bearer {SECRET}");
    // A page on another origin, even holding the token, gets nothing and no CORS headers.
    let r = request(
        srv.addr,
        "GET",
        "/api/v1/health",
        &[("Authorization", &auth), ("Origin", "https://evil.example")],
    )
    .await;
    assert_error(&r, 403, "forbidden");
    assert!(r.header("access-control-allow-origin").is_none());
    // A preflight is refused too.
    let pre = request(
        srv.addr,
        "OPTIONS",
        "/api/v1/health",
        &[
            ("Origin", "https://evil.example"),
            ("Access-Control-Request-Method", "GET"),
            ("Access-Control-Request-Headers", "authorization"),
        ],
    )
    .await;
    assert_error(&pre, 403, "forbidden");
    assert!(pre.header("access-control-allow-origin").is_none());
    // Another local port is another origin.
    let local = request(
        srv.addr,
        "GET",
        "/api/v1/health",
        &[("Authorization", &auth), ("Origin", "http://127.0.0.1:1")],
    )
    .await;
    assert_error(&local, 403, "forbidden");
    // DNS rebinding: a loopback-resolving attacker name.
    let rebind = request(
        srv.addr,
        "GET",
        "/api/v1/health",
        &[("Authorization", &auth), ("Host", "evil.example")],
    )
    .await;
    assert_error(&rebind, 403, "forbidden");
    // No Origin (curl, the CLI) and the server's own origin are fine, and never get CORS headers.
    let own = format!("http://{}", srv.addr);
    let ok = request(
        srv.addr,
        "GET",
        "/api/v1/health",
        &[("Authorization", &auth), ("Origin", &own)],
    )
    .await;
    assert_eq!(ok.status, 200);
    assert!(ok.header("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn openapi_lists_every_registered_route() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(dir.path().to_path_buf(), Token::new(SECRET));
    let api = build_with(state, extra);
    assert!(api.routes.iter().any(|r| r.path == "/api/v1/health"));
    assert!(api.routes.iter().any(|r| r.path == "/api/v1/openapi.json"));
    let srv = start(dir.path()).await;
    let r = get_auth(srv.addr, "/api/v1/openapi.json").await;
    assert_eq!(r.status, 200, "{}", r.body);
    let doc = r.json();
    assert!(doc["openapi"].as_str().unwrap().starts_with("3."));
    let paths = doc["paths"].as_object().unwrap();
    for route in &api.routes {
        let item = paths
            .get(&route.path)
            .unwrap_or_else(|| panic!("{} missing from the OpenAPI document", route.path));
        for m in &route.methods {
            assert!(
                item.get(m.to_lowercase()).is_some(),
                "{m} {} missing",
                route.path
            );
        }
    }
    assert_eq!(
        paths.len(),
        api.routes.len(),
        "the document lists a route that is not registered"
    );
    assert!(doc["components"]["schemas"]["ErrorBody"].is_object());
    assert!(doc["components"]["schemas"]["Health"].is_object());
    assert!(doc["components"]["securitySchemes"]["bearer"].is_object());
    // Every documented route is actually served (not a 404 route-not-found).
    for route in api.routes.iter().filter(|r| !r.path.contains('{')) {
        let resp = get_auth(srv.addr, &route.path).await;
        let unrouted = resp.status == 404 && resp.json()["error"]["code"] == "not_found";
        assert!(!unrouted, "{} is documented but not served", route.path);
    }
}

#[test]
#[should_panic(expected = "registered twice")]
fn registering_a_path_twice_is_a_start_up_panic() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(dir.path().to_path_buf(), Token::new(SECRET));
    let _ = build_with(state, |api| {
        api.route(
            "/health",
            get(|| async { "x" }),
            doc::get("dup", "dup", "Health"),
        )
    });
}

#[tokio::test]
async fn only_loopback_can_be_bound() {
    for host in ["0.0.0.0", "192.168.1.5", "::", "example.com", ""] {
        let e = orch_api::bind(host, 0).await.unwrap_err();
        assert!(matches!(e, orch_api::BindError::NotLoopback(_)), "{host}");
        assert!(e.to_string().contains("loopback"));
    }
    for host in ["127.0.0.1", "localhost", "::1"] {
        if let Ok(l) = orch_api::bind(host, 0).await {
            assert!(l.local_addr().unwrap().ip().is_loopback());
        }
    }
}

#[tokio::test]
async fn shutdown_lets_in_flight_work_finish_and_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(dir.path().to_path_buf(), Token::new(SECRET));
    let listener = orch_api::bind("127.0.0.1", 0).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    let task = tokio::spawn(orch_api::serve(listener, state, async move {
        let _ = rx.await;
    }));
    assert_eq!(get_auth(addr, "/api/v1/health").await.status, 200);
    tx.send(()).unwrap();
    task.await.unwrap().unwrap();
    assert!(
        TcpStream::connect(addr).await.is_err(),
        "still listening after shutdown"
    );
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        self.task.abort();
    }
}
