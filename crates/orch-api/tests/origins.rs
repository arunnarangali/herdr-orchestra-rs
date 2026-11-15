//! `with_allowed_origins`: the desktop app's page origin may call the API (with the token);
//! every other origin is still refused, and nothing changes when no origin is listed.

use orch_api::{AppState, Token, build, with_allowed_origins};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ALLOWED: &str = "tauri://localhost";

async fn start(allowed: &[&str]) -> (std::net::SocketAddr, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(dir.path().to_path_buf(), Token::new(SECRET));
    let listener = orch_api::bind("127.0.0.1", 0).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = with_allowed_origins(
        build(state).router,
        allowed.iter().map(|s| s.to_string()).collect(),
    );
    tokio::spawn(async move { axum::serve(listener, router).await });
    (addr, dir)
}

/// `(status, lower-cased head, body)`.
async fn send(addr: std::net::SocketAddr, method: &str, extra: &[(&str, &str)]) -> (u16, String) {
    let mut req =
        format!("{method} /api/v1/health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (k, v) in extra {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let raw = String::from_utf8_lossy(&raw).to_ascii_lowercase();
    let status = raw.split(' ').nth(1).unwrap().parse().unwrap();
    (status, raw)
}

fn bearer() -> String {
    format!("Bearer {SECRET}")
}

#[tokio::test]
async fn listed_origin_gets_a_preflight_answer_and_cross_origin_headers() {
    let (addr, _d) = start(&[ALLOWED]).await;
    let (status, head) = send(
        addr,
        "OPTIONS",
        &[
            ("Origin", ALLOWED),
            ("Access-Control-Request-Method", "GET"),
            ("Access-Control-Request-Headers", "authorization"),
        ],
    )
    .await;
    assert_eq!(status, 204);
    assert!(head.contains("access-control-allow-origin: tauri://localhost"));
    assert!(
        head.contains("access-control-allow-headers: authorization, content-type, last-event-id")
    );

    let auth = bearer();
    let (status, head) = send(
        addr,
        "GET",
        &[("Origin", ALLOWED), ("Authorization", &auth)],
    )
    .await;
    assert_eq!(status, 200);
    assert!(head.contains("access-control-allow-origin: tauri://localhost"));
    assert!(head.contains("vary: origin"));
}

#[tokio::test]
async fn listed_origin_still_needs_the_token() {
    let (addr, _d) = start(&[ALLOWED]).await;
    let (status, head) = send(addr, "GET", &[("Origin", ALLOWED)]).await;
    assert_eq!(status, 401);
    // The page can read the refusal (it needs to tell a bad token from no server).
    assert!(head.contains("access-control-allow-origin: tauri://localhost"));
}

#[tokio::test]
async fn other_origins_are_refused_with_or_without_a_list() {
    for list in [&[ALLOWED][..], &[][..]] {
        let (addr, _d) = start(list).await;
        let auth = bearer();
        let (status, head) = send(
            addr,
            "GET",
            &[("Origin", "http://evil.example"), ("Authorization", &auth)],
        )
        .await;
        assert_eq!(status, 403);
        assert!(!head.contains("access-control-allow-origin"));
    }
    // Without a list even the desktop origin is refused: nothing is opened by default.
    let (addr, _d) = start(&[]).await;
    let auth = bearer();
    let (status, _) = send(
        addr,
        "GET",
        &[("Origin", ALLOWED), ("Authorization", &auth)],
    )
    .await;
    assert_eq!(status, 403);
}
