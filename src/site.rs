//! PerYourHealth on www.peryourhealth.com: what `auth login --browser` reads
//! out of the Chrome window, and the one read that confirms a session before
//! it is stored.
//!
//! The sign-in starts on the site's own page, where the patient enters the
//! PerYourHealth ID printed on a statement, then continues to One Healthcare
//! ID (identity.onehealthcareid.com), Optum's sign-in, and comes back to the
//! signed-in pages under `/Full/`. The site sets cookies before anyone signs
//! in, so a cookie in the jar proves nothing: the capture reads the whole
//! www.peryourhealth.com jar and [`confirm_session`] makes one read with it,
//! the services page, which answers a visitor with a redirect to the sign-in
//! page and a signed-in patient with the page itself (docs/api.md).

use std::io::Read;
use std::time::Duration;

use pk_cli_browser::{BrowserError, Capture, Captured, Confirm, HostRule};
use pk_cli_core::CliError;
use reqwest::header::{HeaderValue, ACCEPT, CONTENT_TYPE, COOKIE};

use crate::cli::BIN;

/// The provider's name in the browser crate's status lines and errors.
pub const LABEL: &str = "PerYourHealth";
/// The host whose pages the capture reads, and whose jar is the session.
pub const HOST: &str = "www.peryourhealth.com";
/// Where the Chrome window opens: the site's sign-in page.
pub const START_URL: &str = "https://www.peryourhealth.com/Account/Login";
/// What signing in involves, for the opening status line.
pub const HINT: &str = "the PerYourHealth ID from a statement, then your One Healthcare ID, password and code";

/// The read that shows whether a cookie jar is a signed-in patient's: the
/// services page, `GET`, no redirect followed.
pub const SESSION_CHECK_URL: &str = "https://www.peryourhealth.com/Full/Services";

/// The bound on the whole read, connecting included. The browser crate's poll
/// (and Ctrl-C) waits for it.
const TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The most of an answer's body read for [`judge`].
const MAX_BODY: u64 = 4 * 1024 * 1024;

/// What `auth login --browser` reads: a page on [`HOST`], its whole cookie
/// jar and the browser's user agent. With `verify`, the tab stays open until
/// [`confirm_candidate`] says the jar is a signed-in one.
pub fn capture(verify: bool) -> Result<Capture, BrowserError> {
    let capture = Capture::new(LABEL, HostRule::exact(HOST)?)
        .cookie_jar(HOST)
        .with_user_agent()
        .hint(HINT);
    Ok(if verify {
        capture.confirm(confirm_candidate)
    } else {
        capture
    })
}

/// The capture's confirm hook: [`confirm_session`] on the candidate's jar.
pub fn confirm_candidate(candidate: &Captured) -> Confirm {
    match candidate.cookie_header() {
        Some(header) => confirm_session(header.expose(), candidate.user_agent()),
        None => Confirm::NotYet("the browser holds no www.peryourhealth.com cookie yet".into()),
    }
}

/// One read of [`SESSION_CHECK_URL`] with `cookie_header`, judged by
/// [`judge`]. A 429, a 5xx, a 403 with an HTML page, no answer or an answer
/// that does not arrive whole is `Undecided`. A redirect is `NotYet`.
/// `user_agent` is the browser's, for a session captured from one.
pub fn confirm_session(cookie_header: &str, user_agent: Option<&str>) -> Confirm {
    confirm_at(SESSION_CHECK_URL, cookie_header, user_agent)
}

/// [`confirm_session`] against `base` (`client::base`): the real site, or a
/// test's loopback server.
pub fn confirm_session_at(base: &str, cookie_header: &str, user_agent: Option<&str>) -> Confirm {
    if base == crate::client::BASE {
        return confirm_session(cookie_header, user_agent);
    }
    confirm_at(&format!("{base}{}", crate::client::SERVICES), cookie_header, user_agent)
}

/// How the services page shows a signed-in patient.
///
/// `Ready` for an HTML page that [`crate::pages::is_signed_in`] recognizes
/// as a signed-in page. `NotYet` for a 4xx, or the sign-in page itself.
/// Undecided for a 2xx that is not HTML or is neither. It sees only answers
/// that [`confirm_session`] has not already decided: no 429, 5xx, 403 with
/// an HTML page or redirect. `body` is at most 4 MiB.
pub fn judge(status: u16, content_type: Option<&str>, body: &[u8]) -> Confirm {
    let what = format!("GET {}", path_of(SESSION_CHECK_URL));
    if (400..500).contains(&status) {
        return Confirm::NotYet(format!("{what} answered HTTP {status}: not signed in"));
    }
    if !(200..300).contains(&status) {
        return Confirm::undecided(format!("{what} answered HTTP {status}"));
    }
    if !content_type.is_some_and(|t| t.contains("html")) {
        return Confirm::undecided(format!("{what} answered HTTP {status} without HTML"));
    }
    let page = String::from_utf8_lossy(body);
    if crate::pages::is_sign_in_page(&page) {
        return Confirm::NotYet(format!("{what} answered the sign-in page: not signed in"));
    }
    if crate::pages::is_signed_in(&page) {
        return Confirm::Ready;
    }
    Confirm::undecided(format!(
        "{what} answered a page that is neither signed in nor the sign-in page"
    ))
}

/// [`confirm_session`]'s read of `url`, apart from the constant so the tests
/// can point it at a local server.
fn confirm_at(url: &str, cookie_header: &str, user_agent: Option<&str>) -> Confirm {
    let what = format!("GET {}", path_of(url));
    let client = match client(user_agent) {
        Ok(client) => client,
        Err(e) => return Confirm::Fail(e),
    };
    let Ok(mut cookie) = HeaderValue::from_str(cookie_header) else {
        return Confirm::Fail(CliError::Other(
            "the session's Cookie header is not a valid HTTP header value".into(),
        ));
    };
    cookie.set_sensitive(true);
    let answer = match client
        .get(url)
        .header(COOKIE, cookie)
        .header(ACCEPT, "text/html")
        .send()
    {
        Ok(answer) => answer,
        Err(e) if e.is_timeout() => return Confirm::undecided(format!("{what} timed out")),
        Err(_) => return Confirm::undecided(format!("{what} got no answer")),
    };
    if let Some(said) = pk_cli_http::inconclusive(&answer) {
        return Confirm::Undecided {
            reason: format!("{what} answered HTTP {}", said.status),
            retry_after: said.retry_after,
        };
    }
    let status = answer.status().as_u16();
    if answer.status().is_redirection() {
        return Confirm::NotYet(format!(
            "{what} answered with a redirect (HTTP {status}): not signed in"
        ));
    }
    let content_type = answer
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut body = Vec::new();
    if answer.take(MAX_BODY + 1).read_to_end(&mut body).is_err() {
        return Confirm::undecided(format!("{what} answered HTTP {status} and its body did not arrive"));
    }
    if body.len() as u64 > MAX_BODY {
        return Confirm::undecided(format!("{what} answered with more than 1 MiB"));
    }
    judge(status, content_type.as_deref(), &body)
}

/// The confirm read's client: no redirects followed, bounded in time.
fn client(user_agent: Option<&str>) -> Result<reqwest::blocking::Client, CliError> {
    let mut builder = pk_cli_http::builder(BIN, env!("CARGO_PKG_VERSION"))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT);
    if let Some(agent) = user_agent {
        builder = builder.user_agent(agent);
    }
    builder
        .build()
        .map_err(|e| CliError::Other(format!("building the HTTP client: {}", e.without_url())))
}

/// A URL's path without its query, for a reason: reasons never carry a query.
pub(crate) fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = rest.find('/').map_or("/", |i| &rest[i..]);
    path.split(['?', '#']).next().unwrap_or("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pk_cli_browser::JarCookie;
    use std::io::Write;
    use std::net::TcpListener;

    /// The capture `pk-cli-browser` checks before Chrome starts, checked
    /// offline, with and without the confirm hook.
    #[test]
    fn the_browser_capture_is_well_formed() {
        for verify in [true, false] {
            let capture = capture(verify).unwrap();
            capture.validate().unwrap();
            assert_eq!(capture.label(), LABEL);
        }
        assert!(START_URL.starts_with(&format!("https://{HOST}/")));
    }

    /// The session check stays on the site's own host, over https.
    #[test]
    fn the_session_check_is_an_https_read_on_the_sites_host() {
        let host = SESSION_CHECK_URL
            .strip_prefix("https://")
            .unwrap()
            .split(['/', '?', ':'])
            .next()
            .unwrap();
        assert_eq!(host, HOST);
    }

    fn candidate(jar: bool) -> Captured {
        let built = Captured::for_test(LABEL, HOST).user_agent("Mozilla/5.0 (test)");
        if jar {
            built.jar([
                JarCookie::for_test("dummy_visitor", "dummy-visitor", "www.peryourhealth.com", false, "/"),
                JarCookie::for_test("dummy_session", "dummy-value", "www.peryourhealth.com", true, "/"),
            ])
        } else {
            built
        }
        .build()
    }

    /// A candidate without a jar is not read at all.
    #[test]
    fn no_jar_is_not_signed_in_yet() {
        match confirm_candidate(&candidate(false)) {
            Confirm::NotYet(reason) => assert!(reason.contains("no www.peryourhealth.com cookie"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    const SIGNED_IN: &[u8] = br#"<nav><a href="/Full/Services">Services</a></nav>"#;
    const SIGN_IN: &[u8] = br#"<form id="accountIdForm"><input name="PatientIdEntry"></form>"#;

    /// What the site answers (docs/api.md), judged.
    #[test]
    fn judge_reads_the_sites_answers() {
        let html = Some("text/html; charset=utf-8");
        assert!(matches!(judge(200, html, SIGNED_IN), Confirm::Ready));
        assert!(matches!(judge(200, html, SIGN_IN), Confirm::NotYet(r) if r.contains("sign-in page")));
        assert!(matches!(judge(401, html, b""), Confirm::NotYet(r) if r.contains("HTTP 401")));
        for (status, mime, body) in [
            (200, Some("application/json"), &b"{}"[..]),
            (200, html, &b"<p>edge</p>"[..]),
            (200, None, SIGNED_IN),
            (304, None, &b""[..]),
        ] {
            assert!(
                matches!(judge(status, mime, body), Confirm::Undecided { .. }),
                "{status} {:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn a_reason_names_the_path_without_its_query() {
        assert_eq!(path_of("https://www.peryourhealth.com/Full/x?id=1#y"), "/Full/x");
        assert_eq!(path_of("https://www.peryourhealth.com"), "/");
        assert_eq!(path_of("http://127.0.0.1:9/a/b"), "/a/b");
    }

    /// Serves one canned answer on loopback and hands back the request's head.
    fn serve_once(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/check?q=1", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap() == 1 {
                head.push(byte[0]);
            }
            stream.write_all(answer.as_bytes()).unwrap();
            String::from_utf8_lossy(&head).to_lowercase()
        });
        (url, handle)
    }

    /// The read's own decisions, before [`judge`]: what cannot decide is
    /// `Undecided` (with the provider's `Retry-After`), a redirect is
    /// `NotYet`, and the rest reaches `judge`. The request carries the jar and
    /// the browser's user agent, and follows no redirect.
    #[test]
    fn the_confirm_read_decides_what_says_nothing_about_the_session() {
        let undecided = |answer: &'static str, retry: Option<u64>| {
            let (url, server) = serve_once(answer);
            match confirm_at(&url, "a=b; c=d", Some("Mozilla/5.0 (test)")) {
                Confirm::Undecided { reason, retry_after } => {
                    assert!(reason.starts_with("GET /api/check answered HTTP"), "{reason}");
                    assert!(!reason.contains("q=1"), "{reason}");
                    assert_eq!(retry_after, retry.map(Duration::from_secs));
                }
                other => panic!("{answer:?}: {other:?}"),
            }
            server.join().unwrap()
        };
        let head = undecided("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n", None);
        assert!(head.starts_with("get /api/check?q=1 "), "{head}");
        assert!(head.contains("\r\ncookie: a=b; c=d\r\n"), "{head}");
        assert!(head.contains("\r\nuser-agent: mozilla/5.0 (test)\r\n"), "{head}");
        undecided(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 30\r\nContent-Length: 0\r\n\r\n",
            Some(30),
        );
        undecided(
            "HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nContent-Length: 9\r\n\r\n<p>no</p>",
            None,
        );

        let (url, server) =
            serve_once("HTTP/1.1 302 Found\r\nLocation: https://example.com/sign-in.html\r\nContent-Length: 0\r\n\r\n");
        match confirm_at(&url, "a=b", None) {
            Confirm::NotYet(reason) => assert!(reason.contains("redirect (HTTP 302)"), "{reason}"),
            other => panic!("{other:?}"),
        }
        let head = server.join().unwrap();
        assert!(head.contains(&format!("\r\nuser-agent: {BIN}/")), "{head}");

        let (url, server) = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 36\r\n\r\n<a href=\"/Full/Services\">Services</a>",
        );
        assert!(matches!(confirm_at(&url, "a=b", None), Confirm::Ready));
        server.join().unwrap();
    }

    #[test]
    fn no_answer_is_undecided() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        match confirm_at(&format!("http://127.0.0.1:{port}/api/check"), "a=b", None) {
            Confirm::Undecided { reason, .. } => assert_eq!(reason, "GET /api/check got no answer"),
            other => panic!("{other:?}"),
        }
    }
}
