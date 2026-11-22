# Changelog

## Unreleased

- `peryourhealth`: a patient's bills from PerYourHealth (Optum), read-only.
- `profiles list|add|remove`; every command that reads the site takes
  `--profile`.
- `auth login --browser` signs in through a Chrome window in the profile's
  own browser profile (PerYourHealth ID, then One Healthcare ID);
  `auth status|logout|reset`.
- `api GET <PATH>` reads a signed-in page under `/Full/`; payment, sign-in
  and settings paths are refused before a request.
- `config path|show`, `info`, `completions`, `self-update`.
- The repository moved to own-your-health
  (github.com/own-your-health/peryourhealth-cli); `self-update`, `cargo
  install --git` and the medical-common dependency (still v0.6.9) follow it.
- `url` 2.5.9, for RUSTSEC-2026-0592 (a stack overflow in `Url::origin()`).
- CI's `security` job runs the gitleaks CLI 8.30.1, pinned by checksum, over
  the whole history: gitleaks-action needs a paid license on an
  organization's repository.
- The bill model (`src/bill.rs`, docs/fhir.md) follows medrec's bill
  mapping, as flywire's does: every `Money` value is a JSON number in the
  bill's decimal spelling (`125.00`, kept by serde_json's
  `arbitrary_precision`); a bill without lines holds its date of service
  in a ChargeItem of its own; `totalPriceComponent` is the bill's
  breakdown, its billed amount the `base` `billed` component, and
  `totalGross` equals `totalNet`, what the patient owes; a payment's
  detail names the provider paid (`payee`), with the bill's amount when
  it paid one bill alone, and `paymentIssuer` is dropped; statements and
  receipts are typed in `urn:pk-med:doc-type`, and a statement's
  `context.related` names its Account.
