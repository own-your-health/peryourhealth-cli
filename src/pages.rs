//! What PerYourHealth's pages say, read from their HTML (docs/api.md).
//!
//! The site renders every page on the server, so the data is in the HTML.
//! These readers look only at structure (form ids, field names, links), and
//! their errors name what was missing, never a value from the page.

/// Whether `html` is the sign-in page: its account-ID form
/// (`#accountIdForm`, field `PatientIdEntry`). The site answers a signed-out
/// session's request for a signed-in page with a redirect to it.
pub fn is_sign_in_page(html: &str) -> bool {
    html.contains("id=\"accountIdForm\"") && html.contains("name=\"PatientIdEntry\"")
}

/// Whether `html` is a signed-in page: one that is not the sign-in page and
/// links to the signed-in pages under `/Full/`, which a visitor is never
/// served.
pub fn is_signed_in(html: &str) -> bool {
    !is_sign_in_page(html) && html.to_ascii_lowercase().contains("href=\"/full/")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGN_IN: &str = r#"<form id="accountIdForm" method="post"><input type="text" id="PatientIdEntry" name="PatientIdEntry" value="" /></form>"#;
    const SIGNED_IN: &str = r#"<nav><a href="/Full/Services">Services</a></nav><main>...</main>"#;

    #[test]
    fn the_sign_in_page_is_known_by_its_form() {
        assert!(is_sign_in_page(SIGN_IN));
        assert!(!is_signed_in(SIGN_IN));
        assert!(!is_sign_in_page(SIGNED_IN));
        assert!(is_signed_in(SIGNED_IN));
        assert!(!is_signed_in("<p>Request blocked.</p>"));
    }
}
