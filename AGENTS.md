# AGENTS.md — peryourhealth-cli

`peryourhealth` is a read-only CLI for a patient's medical bills on
PerYourHealth (www.peryourhealth.com), the patient billing site of Optum's
revenue-cycle service: each billing account's balances, the services billed
by date with charges, insurance payments, adjustments and what the patient
owes, payments made, and statement PDFs. It reads the site's signed-in
pages, signed in per profile, and emits FHIR for the household's
medical-records system (`records-stream/v1`, `medrec import --from
peryourhealth`). It conforms to piekstra-cli/1 and the `health-records/v1`
profile, on the `pk-cli-*` crates from
[cli-common](https://github.com/piekstra/cli-common) and `pk-med-records`
from [medical-common](https://github.com/own-your-health/medical-common). Every
`pk-cli-*` stays on the cli-common tag medical-common pins.

## Layout

| File | Owns |
|---|---|
| `src/cli.rs` | the clap tree |
| `src/app.rs` | dispatch, argument checks, profiles, config, info, output |
| `src/auth.rs` | `auth login`, `status`, `logout` of one `--profile`, `auth reset`: the browser-profile seams |
| `src/session.rs` | `SessionStore`: get, put and delete a profile's session in the vault; the only code that knows where it lives |
| `src/site.rs` | the sign-in: the browser capture and the session check (`SESSION_CHECK_URL`, `judge`) |
| `src/client.rs` | the reads: pacing, the cookie jar, `refuse` (answers to exit codes), and `api_path`, which checks an `api` path is a read before it is sent |
| `src/pages.rs` | what the site's HTML says: the sign-in page, a signed-in page |
| `src/bill.rs` | the bill model (accounts, bills and their charge lines, payments, statements, receipts) and its FHIR mapping (`docs/fhir.md`). The same file is in flywire-cli, peryourhealth-cli and embryooptions-cli, differing only in the CLI's name in two doc comments: a change to it is made in all three. It needs serde_json's `arbitrary_precision` (Cargo.toml) to keep money's decimal spelling; its unit test fails without it |
| `src/portal.rs` | the signed-in commands (`Env`: where sessions are kept and which site is read; `Site`, which writes refreshed cookies back) |
| `docs/api.md` | the site as observed, and the traps |

## Medical data rules

These come from the household's plan and are not negotiable.

- The repository carries no personal data, in code, fixtures, docs, commits,
  pull requests or issues: no account numbers, PerYourHealth IDs, statement
  or transaction numbers, names, providers the household uses, amounts or
  dates of service. Fixtures are synthetic; `tests/no_personal_data.rs`
  scans every tracked file for the shapes personal data takes.
- Records and their contents are never sent to an outside service: no
  health-data aggregator, cloud OCR or other company's API. A Claude session
  is not one of these (the household's decision, 2026-11-17). A session may
  read, print and summarize the household's real bills when the work needs
  it: debugging a real export, mapping the site with real answers, checking
  a pull. Real data stays out of tracked files, commits, pull requests and
  issues. A raw capture is deleted from `/tmp` once it has served.
- No FHIR or SMART libraries, no aggregators. The FHIR here is mapped by
  hand in `src/bill.rs`; general-purpose crates are fine.
- Read-only. Nothing writes to PerYourHealth: every request is a `GET` of a
  signed-in page or a statement file. It never pays, schedules or cancels a
  payment, stores or changes a payment method, enrolls in autopay or a
  payment plan, changes paperless or contact preferences, sends a message
  or disputes a charge. A write would need SPEC §1.3's confirmation gate and
  a change to this paragraph in the same commit; a step on the site that
  would do any of these is the owner's decision, never the CLI's.

## Profiles and the session

Every command that reads the site takes `--profile <NAME>`
(`PERYOURHEALTH_PROFILE`), which must be configured (`profiles add`); a
missing or unknown one is exit 2 listing the configured ones, and a lone
profile is never picked (`pk_cli_config::profile::resolve`). A session is the
vault entry `<profile>/session`
(`pk_cli_auth::vault::Vault::for_binary("peryourhealth")`: one keychain item,
`vault-key` in `piekstra.peryourhealth`, and an encrypted file per entry
under `~/.config/peryourhealth/vault/`). A `--browser` sign-in runs in the
profile's own kept Chrome profile (`Launch::profile_name`). `profiles
remove` removes the session and that browser profile before the profile;
`auth reset` is `Vault::clear`.

The sign-in is the site's own: the PerYourHealth ID, then One Healthcare ID
(`identity.onehealthcareid.com`, which refuses a headless browser). The
window `auth login --browser` opens is a plain Chrome with
`--remote-debugging-port=0`, so a session filling the sign-in for the owner
drives it through the port in `DevToolsActivePort` in the profile directory
(the `browser-explore` harness, `--port`). The capture reads the
www.peryourhealth.com jar and confirms it with one read of `/Full/Services`
before storing it.

`client::Client` sends one `GET` at a time, at least `client::PACE` apart,
follows no redirect, and maps answers to exit codes: a 401, a redirect or
the sign-in page is 3 (`auth login --browser --profile <NAME> --overwrite`);
a 404 is 4; a 403 page, 429, 5xx and any other status are 5. A `Set-Cookie` updates the jar, written back
once the command has read what it needed, unless a newer sign-in replaced
the stored session meanwhile (`portal::Site::finish`). A failed write-back
is a warning.

## Tests are offline, always

`cargo test` reads no real keychain item or browser profile, stores no
session in the real vault, and contacts nothing beyond loopback. Every test
that runs a credential-reading command sets
`PERYOURHEALTH_KEYCHAIN_SERVICE=piekstra.peryourhealth.hermetic-tests`, a
service that holds nothing, with a temp `XDG_CONFIG_HOME`.

- `tests/cli_surface.rs`: every `--help`, exit codes, JSON errors, argument
  checks before a session is read, profile selection, `NO_COLOR`.
- `tests/site_e2e.rs`: the reads in-process against a loopback server, the
  session in an in-memory vault: the requests, write-back, and the refusals.
- `tests/no_personal_data.rs`.

`make verify` = fmt-check, clippy, test, smoke.

`PERYOURHEALTH_BASE_URL` points every read at a test's server instead of
www.peryourhealth.com. It is honored only for a plain `http` origin on a
loopback address, and only over a test's own store: a
`PERYOURHEALTH_KEYCHAIN_SERVICE` of `piekstra.peryourhealth.<suffix>` and an
`XDG_CONFIG_HOME` away from `~/.config` (`session::is_test_store`), so no
session the owner stored can be sent to it; `auth login --browser` refuses
it. Under `PERYOURHEALTH_KEYCHAIN_SERVICE` (`session::hermetic`) the override
is required: with no `PERYOURHEALTH_BASE_URL`, every command that would send a
request is exit 2 before sending it (`client::base_from`), so a test run that
forgets the server never reaches www.peryourhealth.com.
