//! The PerYourHealth reads, made with a profile's stored session
//! (docs/api.md).
//!
//! - Every read is a `GET` of a page the site's own browser session reads:
//!   the signed-in pages under `/Full/` (HTML) and the statement PDFs.
//! - A request carries the session's `Cookie` header and the browser's user
//!   agent captured with it.
//! - It follows no redirect and is bounded in time: 30 s for the whole
//!   answer, 10 s to connect.
//! - One request at a time, at least [`PACE`] after the last answer.
//! - Nothing writes to PerYourHealth. `peryourhealth api` sends only `GET`s
//!   of the signed-in pages, checked by [`api_path`] before one is sent.
//! - [`refuse`] turns what is no page into an error: a 401, a redirect (the
//!   site sends a signed-out session to its sign-in page) or the sign-in page
//!   itself is a refused session (exit 3); a 404 is exit 4; a 403 page, a
//!   429, a 5xx and any other status are exit 5.
//! - A `Set-Cookie` on an answer updates the client's jar; the caller writes
//!   the updated session back once the command has read what it needed
//!   ([`Client::updated_cookie_header`]).

use std::cell::{Cell, RefCell};
use std::io::Read as _;
use std::time::{Duration, Instant};

use pk_cli_config::profile::ProfileName;
use pk_cli_core::{CliError, Reason};
use pk_cli_secrets::Secret;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, COOKIE, RETRY_AFTER, SET_COOKIE, USER_AGENT};

use crate::cli::BIN;
use crate::session::Session;

/// The site's origin.
pub const BASE: &str = "https://www.peryourhealth.com";
/// Points the reads at a test's loopback server instead of [`BASE`]
/// ([`base`]).
///
/// Required, and only a loopback origin, while a test's keychain service
/// is set ([`base_from`]).
pub const BASE_URL_ENV: &str = "PERYOURHEALTH_BASE_URL";
/// The least time between one answer and the next request.
pub const PACE: Duration = Duration::from_millis(750);
const TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The most of an answer read.
const MAX_BODY: u64 = 32 * 1024 * 1024;

/// The signed-in services page: the session check (`site::SESSION_CHECK_URL`).
pub const SERVICES: &str = "/Full/Services";

/// The origin this process reads: [`BASE`], unless [`BASE_URL_ENV`] names a
/// test's server, which it must under a test's keychain service.
pub fn base() -> Result<String, CliError> {
    base_from(
        std::env::var(BASE_URL_ENV).ok().as_deref(),
        crate::session::is_test_store(),
        crate::session::hermetic(),
    )
}

/// The refusal of a request under a test's keychain service
/// (`session::hermetic`) with no [`BASE_URL_ENV`]: a test run reads only a
/// server on this machine, never www.peryourhealth.com.
fn hermetic_without_base() -> CliError {
    CliError::Usage(format!(
        "{} is set, so this is a test run, and a test run sends requests only to a server on this \
         machine: set {BASE_URL_ENV} to its loopback origin, like http://127.0.0.1:8080 (unset {} \
         to read www.peryourhealth.com)",
        crate::session::KEYCHAIN_SERVICE_ENV,
        crate::session::KEYCHAIN_SERVICE_ENV
    ))
}

/// The check behind [`base`], on the values passed in. Under a
/// test's keychain service (`hermetic`) the override is required, so a
/// test that forgets it is refused before it sends anything. The override is for
/// a test's server: a plain `http` origin on a loopback address, and only
/// over a test's own session store (`session::is_test_store`), so no
/// session the owner stored is sent to it. An ambient variable sending a
/// real session elsewhere would be a way to steal it.
pub fn base_from(value: Option<&str>, test_store: bool, hermetic: bool) -> Result<String, CliError> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        if hermetic {
            return Err(hermetic_without_base());
        }
        return Ok(BASE.to_string());
    };
    let refuse = |why: &str| {
        CliError::Usage(format!(
            "{BASE_URL_ENV} {why}; it exists only to point tests at a server on this machine"
        ))
    };
    if !test_store {
        return Err(refuse(&format!(
            "needs a test's own session store, so no session the owner stored is sent to it: {} naming a \
             `piekstra.peryourhealth.<suffix>` service, and XDG_CONFIG_HOME set away from ~/.config",
            crate::session::KEYCHAIN_SERVICE_ENV
        )));
    }
    let url = reqwest::Url::parse(value).map_err(|_| refuse("is not a URL"))?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    let plain = url.scheme() == "http"
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    if !(loopback && plain) {
        return Err(refuse(
            "must be an http origin on a loopback address, like http://127.0.0.1:8080",
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// A session's cookies, in the order the header gave them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jar {
    cookies: Vec<(String, String)>,
    changed: bool,
}

impl Jar {
    pub fn parse(header: &str) -> Jar {
        let cookies = header
            .split(';')
            .filter_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                (!name.trim().is_empty()).then(|| (name.trim().to_string(), value.trim().to_string()))
            })
            .collect();
        Jar {
            cookies,
            changed: false,
        }
    }

    pub fn header(&self) -> String {
        self.cookies
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.cookies.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// One `Set-Cookie` value: a new value replaces the cookie's, a new
    /// cookie is added, and an empty value or `Max-Age` of zero or less
    /// removes it. The other attributes (domain, path, expiry) are the
    /// browser's business; the site's are all on its own hosts.
    pub fn apply(&mut self, set_cookie: &str) {
        let mut parts = set_cookie.split(';');
        let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else {
            return;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() {
            return;
        }
        let expired = parts.any(|a| {
            a.trim().split_once('=').is_some_and(|(k, v)| {
                k.trim().eq_ignore_ascii_case("max-age") && v.trim().parse::<i64>().is_ok_and(|n| n <= 0)
            })
        });
        let at = self.cookies.iter().position(|(n, _)| n == name);
        match (at, value.is_empty() || expired) {
            (Some(i), true) => {
                self.cookies.remove(i);
                self.changed = true;
            }
            (Some(i), false) if self.cookies[i].1 != value => {
                self.cookies[i].1 = value.to_string();
                self.changed = true;
            }
            (None, false) => {
                self.cookies.push((name.to_string(), value.to_string()));
                self.changed = true;
            }
            _ => {}
        }
    }

    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// What a read sends and to whom: the session's cookies and user agent.
pub struct Client {
    http: reqwest::blocking::Client,
    base: String,
    jar: RefCell<Jar>,
    pace: Duration,
    last: Cell<Option<Instant>>,
    profile: ProfileName,
}

impl Client {
    /// A client for `profile`'s `session` against `base` ([`BASE`], or a
    /// test's loopback server), `pace` apart.
    pub fn new(session: &Session, profile: &ProfileName, base: &str, pace: Duration) -> Result<Client, CliError> {
        let agent = session
            .user_agent
            .clone()
            .unwrap_or_else(|| format!("{BIN}/{}", env!("CARGO_PKG_VERSION")));
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&agent)
                .map_err(|_| CliError::Other("the stored user agent is not a valid HTTP header value".into()))?,
        );
        // A plain client: no cookie store (the jar is ours), no redirects.
        let http = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| CliError::Other(format!("building the HTTP client: {}", e.without_url())))?;
        Ok(Client {
            http,
            base: base.trim_end_matches('/').to_string(),
            jar: RefCell::new(Jar::parse(session.cookie_header.expose())),
            pace,
            last: Cell::new(None),
            profile: profile.clone(),
        })
    }

    /// `GET path` of a signed-in page: its HTML, after [`refuse`]. The
    /// sign-in page in place of the one asked for is a refused session.
    pub fn page(&self, path: &str) -> Result<String, CliError> {
        let answer = self.get(path, "text/html")?;
        refuse(&answer, &self.profile)?;
        let what = &answer.what;
        if !answer.content_type.as_deref().is_some_and(|t| t.contains("html")) {
            return Err(CliError::Upstream(format!(
                "{what} answered HTTP {} with {}, not a page",
                answer.status,
                answer.content_type.as_deref().unwrap_or("no content type")
            )));
        }
        let page = String::from_utf8_lossy(&answer.body).into_owned();
        if crate::pages::is_sign_in_page(&page) {
            return Err(refused_session(what, "the sign-in page", &self.profile));
        }
        Ok(page)
    }

    /// `GET path` of a file (a statement PDF): its bytes and content type,
    /// after [`refuse`].
    pub fn file(&self, path: &str) -> Result<(Vec<u8>, Option<String>), CliError> {
        let answer = self.get(path, "application/pdf,*/*")?;
        refuse(&answer, &self.profile)?;
        Ok((answer.body, answer.content_type))
    }

    /// `GET path` for `peryourhealth api`: the answer as it came, after the
    /// refusals every read gets.
    pub fn api(&self, path: &str) -> Result<Answer, CliError> {
        let answer = self.get(path, "text/html,application/json;q=0.9,*/*;q=0.8")?;
        refuse(&answer, &self.profile)?;
        if answer.content_type.as_deref().is_some_and(|t| t.contains("html"))
            && crate::pages::is_sign_in_page(&String::from_utf8_lossy(&answer.body))
        {
            return Err(refused_session(&answer.what, "the sign-in page", &self.profile));
        }
        Ok(answer)
    }

    /// The session's cookie header as the answers left it, when one changed it.
    pub fn updated_cookie_header(&self) -> Option<Secret> {
        let jar = self.jar.borrow();
        jar.changed().then(|| Secret::new(jar.header()))
    }

    fn get(&self, path: &str, accept: &str) -> Result<Answer, CliError> {
        // The path without its query: a query can carry an account number.
        let what = format!("GET {}", path.split(['?', '#']).next().unwrap_or(path));
        if let Some(last) = self.last.get() {
            let since = last.elapsed();
            if since < self.pace {
                std::thread::sleep(self.pace - since);
            }
        }
        let mut cookie = HeaderValue::from_str(&self.jar.borrow().header())
            .map_err(|_| CliError::Other("the session's Cookie header is not a valid HTTP header value".into()))?;
        cookie.set_sensitive(true);
        let result = self
            .http
            .get(format!("{}{path}", self.base))
            .header(ACCEPT, accept)
            .header(COOKIE, cookie)
            .send();
        let answer = match result {
            Ok(a) => a,
            Err(e) => {
                self.last.set(Some(Instant::now()));
                return Err(if e.is_timeout() {
                    CliError::Upstream(format!("{what} timed out after {} s", TIMEOUT.as_secs()))
                } else {
                    let host = self.base.split_once("://").map_or(self.base.as_str(), |(_, h)| h);
                    CliError::Upstream(format!("{what} got no answer from {host}"))
                });
            }
        };
        let status = answer.status().as_u16();
        let header = |name| {
            answer
                .headers()
                .get(name)
                .and_then(|v: &HeaderValue| v.to_str().ok())
                .map(str::to_string)
        };
        let content_type = header(CONTENT_TYPE);
        let retry_after = header(RETRY_AFTER);
        {
            let mut jar = self.jar.borrow_mut();
            for v in answer.headers().get_all(SET_COOKIE) {
                if let Ok(v) = v.to_str() {
                    jar.apply(v);
                }
            }
        }
        let mut body = Vec::new();
        let read = answer.take(MAX_BODY + 1).read_to_end(&mut body);
        self.last.set(Some(Instant::now()));
        if read.is_err() {
            return Err(CliError::Upstream(format!(
                "{what} answered HTTP {status} and its body did not arrive"
            )));
        }
        if body.len() as u64 > MAX_BODY {
            return Err(CliError::Upstream(format!(
                "{what} answered with more than {} MiB",
                MAX_BODY >> 20
            )));
        }
        Ok(Answer {
            what,
            status,
            content_type,
            retry_after,
            body,
        })
    }
}

/// Where `peryourhealth api` reads: the signed-in pages.
const API_PREFIX: &str = "/Full/";
/// Words that, as a path segment or a query key, name something other than
/// a read: paying, signing in or out, changing a setting, sending. A `GET`
/// of a form page changes nothing, but `api` keeps clear of them all the
/// same, as peryourhealth's reads do.
const ACTION_WORDS: &[&str] = &[
    "pay",
    "autopay",
    "plan",
    "checkout",
    "wallet",
    "card",
    "bank",
    "method",
    "login",
    "logout",
    "signin",
    "signout",
    "create",
    "cancel",
    "update",
    "delete",
    "edit",
    "save",
    "submit",
    "send",
    "message",
    "paperless",
    "preference",
    "setting",
    "contact",
    "dispute",
    "enroll",
];

/// Whether a path segment or query key is, or starts with, an action word.
/// `Payments` (a list of payments made) is a read; `Pay`, `PayNow`,
/// `PaymentPlan` and `PaymentMethod` are not.
fn names_an_action(word: &str) -> bool {
    let w = word.to_ascii_lowercase();
    if w == "payments" || w == "paymenthistory" {
        return false;
    }
    ACTION_WORDS.iter().any(|a| w.starts_with(a))
}

/// `peryourhealth api GET <PATH>`'s path on [`BASE`], or exit 2. Nothing is
/// sent before this passes.
pub fn api_path(path: &str) -> Result<String, CliError> {
    if path.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(CliError::Usage("the path holds a space or a control character".into()));
    }
    let path = match path.strip_prefix(BASE) {
        Some(rest) => rest,
        None if path.contains("://") => {
            return Err(CliError::Usage(format!(
                "`{BIN} api` reads only {BASE}; give a path (`/Full/...`) or a URL on it"
            )))
        }
        None => path,
    };
    let full = path.to_string();
    if !full.starts_with('/') {
        return Err(CliError::Usage("the path starts with `/`".into()));
    }
    let (route, query) = full.split_once('?').unwrap_or((&full, ""));
    let route_lower = route.to_ascii_lowercase();
    if !route_lower.starts_with(&API_PREFIX.to_ascii_lowercase()) || route.contains("..") || route.contains('%') {
        return Err(CliError::Usage(format!(
            "`{BIN} api` reads the signed-in pages only: paths under {API_PREFIX}"
        )));
    }
    let action = route.split('/').any(names_an_action)
        || query
            .split('&')
            .any(|pair| names_an_action(pair.split('=').next().unwrap_or("")));
    if action {
        return Err(CliError::Usage(format!(
            "{BIN} is read-only: that path names a payment, a sign-in or a change, which `api` never reads"
        )));
    }
    Ok(full)
}

/// An answer, read whole.
pub struct Answer {
    /// `GET /path`, no query.
    pub what: String,
    pub status: u16,
    pub content_type: Option<String>,
    pub retry_after: Option<String>,
    pub body: Vec<u8>,
}

fn login_remedy(profile: &ProfileName) -> String {
    format!("{BIN} auth login --browser --profile {profile} --overwrite")
}

fn refused_session(what: &str, said: &str, profile: &ProfileName) -> CliError {
    CliError::Auth(format!(
        "PerYourHealth refused profile `{profile}`'s session ({what}: {said}): it has signed out or expired"
    ))
    .because(Reason::CredentialRefused)
    .with_remedy(login_remedy(profile))
}

/// The answers that are no data whatever the read: a refused session, the
/// edge's page, too many requests, the site's own failures.
pub fn refuse(a: &Answer, profile: &ProfileName) -> Result<(), CliError> {
    let what = &a.what;
    let status = a.status;
    let html = a.content_type.as_deref().is_some_and(|t| t.contains("html"));
    match status {
        200..=299 => Ok(()),
        401 => Err(refused_session(what, "HTTP 401", profile)),
        300..=399 => Err(refused_session(what, &format!("a redirect, HTTP {status}"), profile)),
        403 if html => Err(CliError::Upstream(format!(
            "the site's edge refused {what} (HTTP 403 with its error page)"
        ))
        .because(Reason::ClientBlocked)
        .with_remedy(format!(
            "sign in again with `{}`, which keeps the browser's user agent",
            login_remedy(profile)
        ))),
        403 => Err(CliError::Upstream(format!("PerYourHealth refused {what} (HTTP 403)")).because(Reason::Forbidden)),
        429 => {
            let e = CliError::Upstream(format!(
                "PerYourHealth answered {what} with HTTP 429: too many requests"
            ));
            Err(match &a.retry_after {
                Some(after) => e.with_remedy(format!("try again after {} s", after.trim())),
                None => e.with_remedy("try again in a few minutes"),
            })
        }
        500..=599 => Err(
            CliError::Upstream(format!("PerYourHealth answered {what} with HTTP {status}"))
                .with_remedy("try again later"),
        ),
        400 => Err(CliError::Upstream(format!(
            "PerYourHealth refused {what} as a bad request (HTTP 400); the site may have changed"
        ))
        .because(Reason::BadRequest)),
        404 => Err(CliError::NotFound(format!("PerYourHealth has no {what} (HTTP 404)"))),
        _ => Err(CliError::Upstream(format!(
            "PerYourHealth answered {what} with HTTP {status}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adult1() -> ProfileName {
        ProfileName::parse("adult1").unwrap()
    }

    #[test]
    fn a_test_run_without_a_base_is_refused_before_any_request() {
        for test_store in [true, false] {
            let e = base_from(None, test_store, true).unwrap_err();
            assert_eq!(e.exit_code(), 2);
            let m = e.to_string();
            assert!(
                m.contains(BASE_URL_ENV) && m.contains(crate::session::KEYCHAIN_SERVICE_ENV),
                "{m}"
            );
        }
        assert_eq!(
            base_from(Some("http://127.0.0.1:8"), true, true).unwrap(),
            "http://127.0.0.1:8"
        );
    }

    #[test]
    fn the_base_is_the_site_unless_a_test_points_it_at_loopback() {
        assert_eq!(base_from(None, false, false).unwrap(), BASE);
        assert_eq!(base_from(Some("  "), false, false).unwrap(), BASE);
        for ok in [
            "http://127.0.0.1:8080",
            "http://127.0.0.1:8080/",
            "http://localhost:9",
            "http://[::1]:9",
        ] {
            let b = base_from(Some(ok), true, false).unwrap();
            assert!(!b.ends_with('/'), "{b}");
        }
        // Without a test's own store, a real session could travel.
        assert_eq!(
            base_from(Some("http://127.0.0.1:8080"), false, false)
                .unwrap_err()
                .exit_code(),
            2
        );
        for bad in [
            "https://www.peryourhealth.com.example.com",
            "http://example.com",
            "http://127.0.0.1:1@example.com",
            "http://user@localhost:8",
            "https://127.0.0.1:8",
            "http://127.0.0.1:8/api",
            "http://127.0.0.1:8/?x=1",
            "http://127.0.0.2:8",
            "127.0.0.1:8",
        ] {
            assert_eq!(base_from(Some(bad), true, false).unwrap_err().exit_code(), 2, "{bad}");
        }
    }

    fn answer(status: u16, content_type: &str, body: &str) -> Answer {
        Answer {
            what: "GET /Full/Services".into(),
            status,
            content_type: Some(content_type.into()),
            retry_after: None,
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn refuse_turns_what_is_no_page_into_errors() {
        assert!(refuse(&answer(200, "text/html", "<p>"), &adult1()).is_ok());
        for (status, mime, exit) in [
            (401, "text/html", 3),
            (302, "text/html", 3),
            (403, "text/html", 5),
            (403, "application/json", 5),
            (429, "text/html", 5),
            (503, "text/html", 5),
            (400, "text/html", 5),
            (404, "text/html", 4),
            (410, "text/html", 5),
        ] {
            let e = refuse(&answer(status, mime, ""), &adult1()).err().unwrap();
            assert_eq!(e.exit_code(), exit, "{status}: {e}");
        }
        let e = refuse(&answer(302, "text/html", ""), &adult1()).err().unwrap();
        assert!(e.to_string().contains("auth login --browser --profile adult1"), "{e}");
    }

    #[test]
    fn set_cookie_updates_the_jar() {
        let mut jar = Jar::parse("a=1; b=2");
        jar.apply("a=1; Path=/");
        assert!(!jar.changed(), "the same value changes nothing");
        jar.apply("a=3; Path=/; Secure");
        jar.apply("c=4");
        jar.apply("b=; Max-Age=0");
        assert!(jar.changed());
        assert_eq!(jar.header(), "a=3; c=4");
        assert_eq!(jar.get("c"), Some("4"));
        jar.apply("c=5; Max-Age=-1");
        assert_eq!(jar.header(), "a=3");
    }

    #[test]
    fn the_api_reads_only_the_signed_in_pages() {
        for ok in [
            "/Full/Services",
            "/full/services",
            "/Full/Statements?page=2",
            "/Full/Payments",
            "https://www.peryourhealth.com/Full/Services",
        ] {
            assert!(api_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "/Account/Login",
            "/Account/Logout",
            "/Full/Pay",
            "/Full/PayNow",
            "/Full/PaymentPlan",
            "/Full/PaymentMethods",
            "/Full/AutoPay",
            "/Full/Messages",
            "/Full/Paperless",
            "/Full/Settings",
            "/Full/Services?update=1",
            "/Full/Services/../../Account/Logout",
            "/Full/%2e%2e/Account",
            "https://example.com/Full/Services",
            "http://www.peryourhealth.com/Full/Services",
            "Full/Services",
            "/Full/Services ?x=1",
            "/",
        ] {
            assert_eq!(api_path(bad).err().unwrap().exit_code(), 2, "{bad}");
        }
    }
}
