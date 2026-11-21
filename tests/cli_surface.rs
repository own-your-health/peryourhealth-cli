//! Black-box tests of the binary's surface. None reaches PerYourHealth:
//! each renders help or info, is refused by argument checks that run before
//! a session is read, or finds no session stored. The config directory is a
//! temp dir, never the developer's, and `PERYOURHEALTH_PROFILE` is cleared.
//!
//! None reaches the real keychain or browser profile:
//! `PERYOURHEALTH_KEYCHAIN_SERVICE` names a service that holds nothing (a missing
//! item needs no approval), and under it the binary never touches the kept
//! profile. `XDG_DATA_HOME`, where Linux keeps that profile, is a temp dir
//! besides. No test stores a session, so no test writes the vault's key or
//! reads one: the vault reads its key only when an entry exists (cli-common
//! DESIGN.md §2.5).
//!
//! The temp config of [`peryourhealth`] configures one profile, `adult1`.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

fn tmp() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "peryourhealth-cli-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The empty keychain service every test reads instead of `piekstra.peryourhealth`.
const HERMETIC_SERVICE: &str = "piekstra.peryourhealth.hermetic-tests";

/// A config directory whose config configures `profiles`.
fn config_with(profiles: &[&str]) -> std::path::PathBuf {
    let base = tmp();
    std::fs::create_dir_all(base.join("peryourhealth")).unwrap();
    let names: serde_json::Map<String, Value> = profiles
        .iter()
        .map(|p| (p.to_string(), serde_json::json!({})))
        .collect();
    std::fs::write(
        base.join("peryourhealth").join("config.json"),
        serde_json::json!({ "profiles": names }).to_string(),
    )
    .unwrap();
    base
}

fn bare() -> Command {
    let mut cmd = Command::cargo_bin("peryourhealth").unwrap();
    cmd.env("PERYOURHEALTH_KEYCHAIN_SERVICE", HERMETIC_SERVICE)
        .env("XDG_DATA_HOME", tmp())
        .env_remove("PERYOURHEALTH_CHROME")
        .env_remove("PERYOURHEALTH_PROFILE")
        .env_remove("PERYOURHEALTH_BASE_URL")
        .env_remove("NO_COLOR");
    cmd
}

fn peryourhealth() -> Command {
    let mut cmd = bare();
    cmd.env("XDG_CONFIG_HOME", config_with(&["adult1"]));
    cmd
}

fn json_of(out: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice(&out.get_output().stdout).expect("one JSON document on stdout")
}

#[test]
fn every_command_renders_help() {
    for args in [
        vec!["--help"],
        vec!["auth", "--help"],
        vec!["auth", "login", "--help"],
        vec!["auth", "status", "--help"],
        vec!["auth", "logout", "--help"],
        vec!["auth", "reset", "--help"],
        vec!["profiles", "--help"],
        vec!["profiles", "list", "--help"],
        vec!["profiles", "add", "--help"],
        vec!["profiles", "remove", "--help"],
        vec!["config", "--help"],
        vec!["config", "path", "--help"],
        vec!["config", "show", "--help"],
        vec!["api", "--help"],
        vec!["self-update", "--help"],
        vec!["completions", "--help"],
        vec!["info", "--help"],
    ] {
        peryourhealth().args(&args).assert().success();
    }
}

#[test]
fn help_says_read_only_and_unofficial() {
    peryourhealth()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Unofficial read-only"));
}

#[test]
fn info_declares_the_profile_and_login_profiles() {
    let out = bare().env("XDG_CONFIG_HOME", tmp()).arg("info").assert().success();
    let v = json_of(&out);
    assert_eq!(v["schema"], "cli-info/v1");
    assert_eq!(v["name"], "peryourhealth");
    let caps = v["capabilities"].as_array().unwrap();
    assert!(caps.iter().any(|c| c == "login-profiles"), "{caps:?}");
    assert!(!caps.iter().any(|c| c == "records-files"), "no files source: {caps:?}");
    assert_eq!(v["auth"]["required"], true);
}

#[test]
fn completions_and_version_run() {
    peryourhealth().args(["completions", "zsh"]).assert().success();
    peryourhealth()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn no_color_is_accepted() {
    peryourhealth().env("NO_COLOR", "1").arg("info").assert().success();
}

/// A command that signs in needs a configured profile: none given is exit 2
/// listing the configured ones, an unknown one too, and a lone profile is
/// never picked.
#[test]
fn a_login_command_needs_its_profile() {
    for args in [
        vec!["auth", "status"],
        vec!["auth", "logout"],
        vec!["api", "GET", "/Full/Services"],
    ] {
        let out = peryourhealth().args(&args).arg("--json").assert().code(2);
        let v = json_of(&out);
        assert_eq!(v["error"]["code"], "usage", "{args:?}");
        let message = v["error"]["message"].as_str().unwrap();
        assert!(message.contains("adult1"), "{args:?}: {message}");
    }
    let out = peryourhealth()
        .args(["api", "GET", "/Full/Services", "--profile", "nobody", "--json"])
        .assert()
        .code(2);
    assert!(json_of(&out)["error"]["message"].as_str().unwrap().contains("adult1"));
    // With none configured, it names `profiles add`.
    let out = bare()
        .env("XDG_CONFIG_HOME", tmp())
        .args(["api", "GET", "/Full/Services", "--json"])
        .assert()
        .code(2);
    assert!(json_of(&out)["error"].to_string().contains("profiles add"));
}

/// The variable stands in for the flag.
#[test]
fn the_profile_variable_is_read() {
    peryourhealth()
        .env("PERYOURHEALTH_PROFILE", "adult1")
        .args(["auth", "status", "--json"])
        .assert()
        .success();
}

#[test]
fn a_command_without_a_login_takes_no_profile() {
    for args in [
        vec!["info"],
        vec!["profiles", "list"],
        vec!["config", "path"],
        vec!["auth", "reset", "--force"],
        vec!["completions", "bash"],
    ] {
        peryourhealth()
            .args(&args)
            .args(["--profile", "adult1"])
            .assert()
            .code(2);
    }
    // The variable is not read by them.
    peryourhealth()
        .env("PERYOURHEALTH_PROFILE", "adult1")
        .arg("info")
        .assert()
        .success();
}

/// A signed-out profile: status says so, and a read is exit 3 naming the
/// sign-in. Nothing is stored, so nothing reads a key.
#[test]
fn a_profile_without_a_session() {
    let out = peryourhealth()
        .args(["auth", "status", "--profile", "adult1", "--json"])
        .assert()
        .success();
    let v = json_of(&out);
    assert_eq!(v["schema"], "auth-status/v1");
    assert_eq!(v["authenticated"], false);
    assert_eq!(v["method"], "browser-session");
    let out = peryourhealth()
        .args(["api", "GET", "/Full/Services", "--profile", "adult1", "--json"])
        .assert()
        .code(3);
    let v = json_of(&out);
    assert_eq!(v["error"]["code"], "auth");
    assert!(
        v["error"].to_string().contains("auth login --browser --profile adult1"),
        "{v}"
    );
    let out = peryourhealth()
        .args(["auth", "logout", "--profile", "adult1", "--json"])
        .assert()
        .success();
    assert_eq!(json_of(&out)["session_removed"], false);
}

/// `api` reads only, and only the signed-in pages, refused before the
/// session is read.
#[test]
fn the_api_is_read_only() {
    for args in [
        vec!["api", "POST", "/Full/Services"],
        vec!["api", "DELETE", "/Full/Services"],
        vec!["api", "GET", "/Account/Logout"],
        vec!["api", "GET", "/Full/PayNow"],
        vec!["api", "GET", "/Full/AutoPay"],
        vec!["api", "GET", "https://example.com/Full/Services"],
    ] {
        peryourhealth()
            .args(&args)
            .args(["--profile", "adult1"])
            .assert()
            .code(2);
    }
}

#[test]
fn profiles_are_added_listed_and_removed() {
    let config = tmp();
    let run = |args: &[&str]| {
        let mut cmd = bare();
        cmd.env("XDG_CONFIG_HOME", &config).args(args);
        cmd
    };
    let out = run(&["profiles", "list", "--json"]).assert().success();
    assert_eq!(
        json_of(&out),
        serde_json::json!({ "schema": "profile-list/v1", "profiles": [] })
    );
    let out = run(&["profiles", "add", "adult2", "--json"]).assert().success();
    assert_eq!(json_of(&out)["added"], true);
    let out = run(&["profiles", "add", "adult2", "--json"]).assert().success();
    assert_eq!(json_of(&out)["added"], false);
    run(&["profiles", "add", "Bad Name"]).assert().code(2);
    let out = run(&["profiles", "list", "--json"]).assert().success();
    assert_eq!(json_of(&out)["profiles"][0]["name"], "adult2");
    // Removing asks; without a terminal it needs --force (exit 6).
    run(&["profiles", "remove", "adult2"]).assert().code(6);
    run(&["profiles", "remove", "nobody", "--force"]).assert().code(4);
    let out = run(&["profiles", "remove", "adult2", "--force", "--json"])
        .assert()
        .success();
    let v = json_of(&out);
    assert_eq!(v["schema"], "profile-remove/v1");
    assert_eq!(v["session_removed"], false);
    assert!(
        v.get("browser_profile_removed").is_none(),
        "left alone under the test service"
    );
    let out = run(&["profiles", "list", "--json"]).assert().success();
    assert!(json_of(&out)["profiles"].as_array().unwrap().is_empty());
}

#[test]
fn auth_login_checks_its_flags_before_anything() {
    for args in [
        vec!["auth", "login"],
        vec!["auth", "login", "--browser", "--stdin"],
        vec!["auth", "login", "--stdin"],
        vec!["auth", "login", "--browser", "--user-agent", "x"],
    ] {
        peryourhealth()
            .args(&args)
            .args(["--profile", "adult1"])
            .assert()
            .code(2);
    }
}

#[test]
fn auth_reset_asks_first() {
    peryourhealth().args(["auth", "reset"]).assert().code(6);
}

#[test]
fn config_shows_the_profiles() {
    let out = peryourhealth().args(["config", "show", "--json"]).assert().success();
    let v = json_of(&out);
    assert_eq!(v["schema"], "config/v1");
    assert!(v["profiles"].get("adult1").is_some(), "{v}");
    peryourhealth()
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::ends_with("config.json\n"));
}

/// `PERYOURHEALTH_BASE_URL` points reads at a test's loopback server only, and
/// only with the test keychain service, refused before any vault is opened.
#[test]
fn the_base_url_override_is_for_loopback_tests_only() {
    for url in [
        "https://www.peryourhealth.com.example.com",
        "http://example.com",
        "http://127.0.0.1:1@example.com",
    ] {
        peryourhealth()
            .env("PERYOURHEALTH_BASE_URL", url)
            .args(["api", "GET", "/Full/Services", "--profile", "adult1"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("PERYOURHEALTH_BASE_URL"));
    }
    peryourhealth()
        .env_remove("PERYOURHEALTH_KEYCHAIN_SERVICE")
        .env("PERYOURHEALTH_BASE_URL", "http://127.0.0.1:9")
        .args(["api", "GET", "/Full/Services", "--profile", "adult1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("PERYOURHEALTH_KEYCHAIN_SERVICE"));
    // The real service name is not a test's, wherever the vault is.
    peryourhealth()
        .env("PERYOURHEALTH_KEYCHAIN_SERVICE", "piekstra.peryourhealth")
        .env("PERYOURHEALTH_BASE_URL", "http://127.0.0.1:9")
        .args(["api", "GET", "/Full/Services", "--profile", "adult1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("piekstra.peryourhealth.<suffix>"));
    peryourhealth()
        .env("PERYOURHEALTH_BASE_URL", "http://127.0.0.1:9")
        .args(["auth", "login", "--browser", "--profile", "adult1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("real site"));
    // A loopback origin under the test service passes; the signed-out
    // profile then stops it at the session, before a request.
    peryourhealth()
        .env("PERYOURHEALTH_BASE_URL", "http://127.0.0.1:9")
        .args(["api", "GET", "/Full/Services", "--profile", "adult1"])
        .assert()
        .code(3);
}
