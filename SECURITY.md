# Security

Report a vulnerability privately through GitHub's "Report a vulnerability"
(Security tab), not in an issue.

## What peryourhealth touches

- Contacts no server except GitHub, for `self-update`, and
  `https://www.peryourhealth.com`: for `auth login`, a Chrome window the patient
  signs in through and one read that checks the session before it is
  stored; for a command run with `--profile`, the patient's own reads. Every
  request is a `GET` of a signed-in page under `/Full/` or a statement file,
  one at a time and spaced apart, with a 30-second limit and no redirect
  followed. Every URL is built from that
  fixed origin. `PERYOURHEALTH_BASE_URL` replaces it for tests only: it is
  refused unless it is a plain `http` origin on 127.0.0.1, localhost or
  `[::1]`, `PERYOURHEALTH_KEYCHAIN_SERVICE` names a test service
  (`piekstra.peryourhealth.<suffix>`) and `XDG_CONFIG_HOME` is set away from
  `~/.config`, so a session the owner stored is never sent anywhere but
  PerYourHealth. `peryourhealth api` is held to the same: a `GET` of a path
  under `/Full/`, refused before anything is sent when it names a payment, a
  sign-in or sign-out, a setting or a message.
- A cookie the site changes is written back to the profile's session once
  the command has read what it needed, unless a newer sign-in replaced the
  stored session meanwhile. A failed write-back is a warning on stderr.
- Stores one credential per profile, its PerYourHealth session: the `Cookie`
  header, the browser's user agent and the capture time, as the vault entry
  `<profile>/session`. The vault (`pk_cli_auth::vault`) is encrypted
  owner-only files under `~/.config/peryourhealth/vault/`, with one key in the
  keychain (service `piekstra.peryourhealth`, account `vault-key`). A session
  never goes on a command line or into a log.
- Keeps one Chrome profile per peryourhealth profile for `auth login --browser`:
  `~/Library/Application Support/piekstra.peryourhealth/browser-profile-<profile>`
  (under `$XDG_DATA_HOME` on Linux), owner-only (`0700`; a launch is refused
  otherwise). `--fresh-profile` signs in without it.
- `auth logout --profile <NAME>` and `profiles remove <NAME>` remove that
  profile's session, then its Chrome profile. `auth reset` removes every
  session and the vault key.
- The config file holds the profile names.

## What peryourhealth never does

- Writes to PerYourHealth: no payment, payment method, autopay, payment
  plan, paperless or contact setting, message, dispute or sign-out request.
- Signs in by itself: the patient's One Healthcare ID password and code are
  typed (or filled) on the sign-in service's own page, never passed to
  peryourhealth.
- Sends bills anywhere: the output goes to stdout or a file you name.
