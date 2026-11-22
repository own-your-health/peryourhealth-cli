//! The repository carries no personal data; runtime output may.
//!
//! Walks every tracked file (`git ls-files`) for the *shapes* personal data
//! takes (phone numbers, long id numbers, street addresses, email
//! addresses) rather than a list of real values, which would publish what
//! it exists to keep out.

use std::process::Command;

fn root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

fn tracked_files() -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files"])
        .current_dir(root())
        .output()
        .expect("git ls-files");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

fn digits_run(s: &str, min: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            cur.push(c);
        } else {
            if cur.len() >= min {
                out.push(cur.clone());
            }
            cur.clear();
        }
    }
    out
}

/// A digit run is a dummy when it is all one digit, zeros then one digit
/// (`00000001`), or a 555 phone number: `555…`, or any area code with the
/// 555-0100 to 555-0199 lines reserved for fiction.
fn is_dummy_number(n: &str) -> bool {
    let first = n.chars().next().unwrap();
    let head = &n[..n.len() - 1];
    let local = n.strip_prefix('1').filter(|r| r.len() == 10).unwrap_or(n);
    let fictional = local.len() == 10 && &local[3..8] == "55501";
    n.chars().all(|c| c == first)
        || head.chars().all(|c| c == '0')
        || n.starts_with("555")
        || n.starts_with("1555")
        || fictional
}

/// Vendors' own sending domains, named in docs/api.md.
const PUBLIC_EMAIL_DOMAINS: &[&str] = &["example.com", "peryourhealth.com", "optum.com"];

/// `line` with each run of 40 or more hex digits blanked: a SHA-1 or
/// SHA-256 digest (a pinned checksum), whose digits read as an account
/// number. Nothing personal is that shape.
fn without_digests(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.len() < 40 {
            out.push_str(run);
        }
        run.clear();
    };
    for c in line.chars() {
        if c.is_ascii_hexdigit() {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// `line` with each `YYYY-MM-DD HH:MM:SS` (or `T` between) blanked: the
/// portal's own time stamps, which read as a phone number once the
/// separators are gone.
fn without_datetimes(line: &str) -> String {
    let b = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < b.len() {
        let is = |j: usize, digits: bool| b.get(i + j).is_some_and(|c| c.is_ascii_digit() == digits);
        let at = |j: usize, c: u8| b.get(i + j) == Some(&c);
        let datetime = (0..4).all(|j| is(j, true))
            && at(4, b'-')
            && is(5, true)
            && is(6, true)
            && at(7, b'-')
            && is(8, true)
            && is(9, true)
            && (at(10, b' ') || at(10, b'T'))
            && is(11, true)
            && is(12, true)
            && at(13, b':')
            && is(14, true)
            && is(15, true)
            && at(16, b':')
            && is(17, true)
            && is(18, true);
        if datetime {
            out.push_str("<datetime>");
            i += 19;
        } else {
            let c = line[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// The personal-data shapes on one line, each as `kind value`.
fn scan_line(line: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let line = without_datetimes(&without_digests(line));
    let line = line.as_str();
    let phoneish = line.replace(['(', ')', '-', '.', ' '], "");
    for n in digits_run(line, 7)
        .into_iter()
        .chain(digits_run(&phoneish, 10).into_iter().filter(|n| n.len() <= 11))
    {
        if !is_dummy_number(&n) {
            findings.push(format!("number {n}"));
        }
    }
    for word in line.split(|c: char| c.is_whitespace() || "\"'<>(),;`".contains(c)) {
        if let Some((local, domain)) = word.split_once('@') {
            let domain = domain.trim_end_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '-'));
            let ok = local.is_empty()
                || !domain.contains('.')
                || PUBLIC_EMAIL_DOMAINS.contains(&domain)
                || domain.ends_with(".example")
                || domain.ends_with("users.noreply.github.com");
            if !ok {
                findings.push(format!("email {word}"));
            }
        }
    }
    let lower = line.to_ascii_lowercase();
    // A street suffix as a whole word ("St", not "status"), anywhere on the
    // line, with a house number in the four words before it.
    let address_like = [" way", " st", " ave", " blvd", " dr", " ln", " ct", " rd"]
        .iter()
        .any(|suffix| {
            lower.match_indices(suffix).any(|(pos, _)| {
                let whole_word = !lower[pos + suffix.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric());
                whole_word
                    && lower[..pos]
                        .split_whitespace()
                        .rev()
                        .take(4)
                        .any(|w| w.len() >= 3 && w.chars().all(|c| c.is_ascii_digit()))
            })
        });
    if address_like && !lower.contains("example st") {
        findings.push(format!("address-like `{}`", line.trim()));
    }
    findings
}

#[test]
fn tracked_files_carry_no_personal_data() {
    let files = tracked_files();
    if files.is_empty() {
        // Not a git checkout (a packaged crate): nothing to scan.
        return;
    }
    let mut findings = Vec::new();
    for file in files {
        if file.ends_with(".lock") || file.starts_with("LICENSE") || file == "tests/no_personal_data.rs" {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(format!("{}/{file}", root())) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            for f in scan_line(line) {
                findings.push(format!("{file}:{}: {f}", i + 1));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "personal-data shapes in tracked files:\n{}",
        findings.join("\n")
    );
}

#[test]
fn no_tracked_file_is_a_raw_capture() {
    for file in tracked_files() {
        let lower = file.to_ascii_lowercase();
        for ext in [".har", ".csv", ".raw.json", ".raw.html", ".ndjson", ".png", ".jpg"] {
            assert!(!lower.ends_with(ext), "{file} is tracked; captures never are");
        }
    }
}

#[test]
fn the_scanner_flags_real_shapes_and_passes_dummies() {
    for real in [
        "call (772) 404-1234",
        "call 888-555-0200",
        "account 9234567812",
        "account ID 1234-Z9234567812",
        "mail someone@gmail.com",
        "lives at 1234 NW Oak St, Somewhere",
        "statement 7342918",
        "call 772-404-1234 at 2025-08-02 12:00:00",
        "the status was 200, then 1234 Oak St",
        "sha 551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb account 9234567812",
        "bad1234567",
    ] {
        assert!(!scan_line(real).is_empty(), "missed: {real}");
    }
    for dummy in [
        "call (555) 010-0001",
        "customer service 888-555-0100",
        "ACCT-0000001",
        "alex@example.com",
        "100 Example St, Exampleton",
        "2025-03-14",
        "\"created\": \"2025-08-02 12:00:00\"",
        "one 401 and 302), drives every command",
        "GITLEAKS_SHA256: 551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb",
    ] {
        assert!(
            scan_line(dummy).is_empty(),
            "false positive: {dummy}: {:?}",
            scan_line(dummy)
        );
    }
}
