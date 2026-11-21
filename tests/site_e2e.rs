//! The reads of PerYourHealth, in-process, against a loopback server, the
//! session in an in-memory vault: the requests' paths and headers, the
//! cookie write-back, and the refusals. Nothing reads the real keychain or
//! reaches the network.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use peryourhealth::portal::{Env, Site};
use peryourhealth::session::{Session, SessionStore};
use pk_cli_auth::vault::{MemoryKeys, Vault};
use pk_cli_config::profile::ProfileName;
use pk_cli_secrets::Secret;

/// One request the server saw.
#[derive(Debug, Clone)]
struct Seen {
    target: String,
    cookie: Option<String>,
    user_agent: Option<String>,
}

/// A loopback server answering each `GET` by its path and query from
/// `routes` (status, content type, body); anything else is a 404.
/// `set_cookie` goes on every answer.
struct Server {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn serve(routes: BTreeMap<String, (u16, &'static str, String)>, set_cookie: Option<&'static str>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                match stream.read(&mut byte) {
                    Ok(1) => head.push(byte[0]),
                    _ => break,
                }
            }
            let head = String::from_utf8_lossy(&head).to_string();
            let target = head.split_whitespace().nth(1).unwrap_or("").to_string();
            let header = |name: &str| {
                head.lines().find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
                })
            };
            log.lock().unwrap().push(Seen {
                target: target.clone(),
                cookie: header("cookie"),
                user_agent: header("user-agent"),
            });
            let (status, mime, body) =
                routes
                    .get(&target)
                    .cloned()
                    .unwrap_or((404, "text/html", "<p>not found</p>".to_string()));
            let mut answer = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            if status == 302 {
                answer.push_str("Location: /Account/Login\r\n");
            }
            if let Some(c) = set_cookie {
                answer.push_str(&format!("Set-Cookie: {c}\r\n"));
            }
            answer.push_str("\r\n");
            answer.push_str(&body);
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    Server { base, seen }
}

/// A signed-in page and the sign-in page, synthetic: only the structure the
/// readers look at (`src/pages.rs`).
const SIGNED_IN: &str =
    r#"<html><body><nav><a href="/Full/Services">Services</a></nav><main>Services</main></body></html>"#;
const SIGN_IN: &str =
    r#"<html><body><form id="accountIdForm" method="post"><input name="PatientIdEntry" /></form></body></html>"#;

fn site_routes() -> BTreeMap<String, (u16, &'static str, String)> {
    let mut r = BTreeMap::new();
    r.insert(
        "/Full/Services".into(),
        (200, "text/html; charset=utf-8", SIGNED_IN.to_string()),
    );
    r.insert("/Full/Data".into(), (200, "application/json", r#"{"a":1}"#.to_string()));
    r.insert("/Full/Expired".into(), (302, "text/html", String::new()));
    r.insert("/Full/SignedOut".into(), (200, "text/html", SIGN_IN.to_string()));
    r
}

fn adult1() -> ProfileName {
    ProfileName::parse("adult1").unwrap()
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "peryourhealth-e2e-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// An environment reading `server`, with adult1's session stored.
fn env(server: &Server, tag: &str) -> Env {
    let store = SessionStore::with_vault(Vault::new(MemoryKeys::new(), temp_dir(tag)));
    store
        .put(
            &adult1(),
            &Session::new(
                Secret::new("dummy_session=1; dummy_other=x"),
                Some("Mozilla/5.0 (test)".into()),
            ),
        )
        .unwrap();
    Env {
        sessions: store,
        base: server.base.clone(),
        pace: Duration::ZERO,
    }
}

#[test]
fn the_api_returns_the_answer_whole_with_the_sessions_headers() {
    let server = serve(site_routes(), None);
    let env = env(&server, "api");
    let site = Site::open(&env, &adult1()).unwrap();
    let path = peryourhealth::client::api_path("/Full/Services").unwrap();
    let a = site.api(&path).unwrap();
    assert_eq!(a.status, 200);
    assert_eq!(String::from_utf8_lossy(&a.body), SIGNED_IN);
    let a = site.api("/Full/Data").unwrap();
    assert_eq!(a.content_type.as_deref(), Some("application/json"));
    let seen = server.seen.lock().unwrap().clone();
    assert_eq!(seen[0].target, "/Full/Services");
    assert_eq!(seen[0].cookie.as_deref(), Some("dummy_session=1; dummy_other=x"));
    assert_eq!(seen[0].user_agent.as_deref(), Some("Mozilla/5.0 (test)"));
}

#[test]
fn a_signed_out_session_is_exit_3() {
    let server = serve(site_routes(), None);
    let env = env(&server, "signed-out");
    let site = Site::open(&env, &adult1()).unwrap();
    for path in ["/Full/Expired", "/Full/SignedOut"] {
        let e = site.api(path).err().unwrap();
        assert_eq!(e.exit_code(), 3, "{path}: {e}");
        assert!(e.to_string().contains("auth login --browser --profile adult1"), "{e}");
    }
}

#[test]
fn no_session_is_exit_3_naming_the_sign_in() {
    let server = serve(site_routes(), None);
    let env = Env {
        sessions: SessionStore::with_vault(Vault::new(MemoryKeys::new(), temp_dir("none"))),
        base: server.base.clone(),
        pace: Duration::ZERO,
    };
    let e = Site::open(&env, &adult1()).err().unwrap();
    assert_eq!(e.exit_code(), 3);
    assert!(
        e.remedy().unwrap().contains("auth login --browser --profile adult1"),
        "{e}"
    );
}

#[test]
fn refreshed_cookies_are_written_back_once() {
    let server = serve(site_routes(), Some("dummy_session=2; Path=/; Secure; HttpOnly"));
    let env = env(&server, "writeback");
    let site = Site::open(&env, &adult1()).unwrap();
    site.api("/Full/Services").unwrap();
    assert!(site.finish().unwrap());
    let stored = env.sessions.get(&adult1()).unwrap().unwrap();
    assert_eq!(stored.cookie_header.expose(), "dummy_session=2; dummy_other=x");
    assert_eq!(stored.user_agent.as_deref(), Some("Mozilla/5.0 (test)"));

    // A sign-in made meanwhile is never overwritten.
    let site = Site::open(&env, &adult1()).unwrap();
    site.api("/Full/Services").unwrap();
    env.sessions
        .put(&adult1(), &Session::new(Secret::new("dummy_session=9"), None))
        .unwrap();
    assert!(!site.finish().unwrap());
    let stored = env.sessions.get(&adult1()).unwrap().unwrap();
    assert_eq!(stored.cookie_header.expose(), "dummy_session=9");
}

#[test]
fn no_answer_names_the_server_read() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let server = Server {
        base: base.clone(),
        seen: Arc::new(Mutex::new(Vec::new())),
    };
    let env = env(&server, "no-answer");
    let e = Site::open(&env, &adult1())
        .unwrap()
        .api("/Full/Services")
        .err()
        .unwrap();
    assert_eq!(e.exit_code(), 5);
    let host = base.trim_start_matches("http://");
    assert!(e.to_string().contains(&format!("got no answer from {host}")), "{e}");
}
