# peryourhealth

Unofficial; not affiliated with or endorsed by Optum, Inc. or PerYourHealth.

`peryourhealth` is a read-only command line for a patient's own medical bills
on [PerYourHealth](https://www.peryourhealth.com), the patient billing site
Optum runs for many physician groups and hospitals. It signs in as you,
reads what the site shows you, and prints it or writes it as FHIR R4 for a
personal medical-records store (`records-stream/v1`, the `health-records/v1`
profile of [medical-common](https://github.com/own-your-health/medical-common),
read by `medrec import --from peryourhealth`). It never pays, schedules or
cancels a payment, changes a setting, or sends a message.

Status: sign-in and `api` work. The bill commands (`accounts`, `bills`,
`payments`, `patients`, `records export`, `documents`) come once the
signed-in pages are mapped (`docs/api.md`).

## Who it is for

Anyone whose statement from a provider names PerYourHealth and prints a
"PerYourHealth ID": you sign in with that ID and a One Healthcare ID, Optum's
sign-in, which the site lets you create on the way.

No developer registration is needed. PerYourHealth publishes no API and no
developer program, so there is no app to register and no key to request:
`peryourhealth` reads the same pages your browser reads once you are signed
in (`docs/api.md`).

## Install

```sh
cargo install --git https://github.com/own-your-health/peryourhealth-cli --locked
# or download a release, peryourhealth-<target>.tar.gz (macOS Apple Silicon or
# Intel, Linux x86_64), put `peryourhealth` on your PATH; later: peryourhealth self-update
```

It needs Google Chrome for the sign-in (`PERYOURHEALTH_CHROME` names another
Chromium-based browser), and the macOS keychain or, on Linux, a Secret
Service keyring such as GNOME Keyring for the stored session.

## Quick start

```sh
peryourhealth profiles add me                          # one profile per One Healthcare ID
peryourhealth auth login --browser --profile me        # a Chrome window opens; sign in there
peryourhealth auth status --profile me
peryourhealth api GET /Full/Services --profile me      # the services page, as the site serves it
```

Every command that reads the site takes `--profile <NAME>`
(`PERYOURHEALTH_PROFILE`); there is no default. `--json` on every command
prints one schema-tagged document.

## What needs a person

`auth login --browser` opens Chrome on PerYourHealth's sign-in page, in a
browser profile kept for that `peryourhealth` profile, and waits up to 10
minutes. On the site's own pages you:

1. enter the PerYourHealth ID printed on your statement (the whole of it,
   with its `*` or `-`) and click Next;
2. click "Continue to One Healthcare ID";
3. sign in to One Healthcare ID (or create one there) with your user name,
   password and the code it sends.

`peryourhealth` never sees your password or the code. Once the site shows
you signed in, it reads the session's cookies, checks them with one read of
the services page, stores them encrypted (`SECURITY.md`) and closes the
window.

How long a sign-in lasts has not been measured yet. When the site has ended
it, a read exits 3 and prints the command that signs in again:
`peryourhealth auth login --browser --profile <NAME> --overwrite`.

## Commands

| Command | Output |
|---|---|
| `profiles list\|add\|remove` | `profile-list/v1` |
| `auth login --browser`, `auth status\|logout\|reset` | `auth-login/v1`, `auth-status/v1`, … |
| `api GET <PATH>` | a signed-in page under `/Full/`, as it came |
| `info` | `cli-info/v1` |

Exit codes: 0 ok, 2 usage, 3 signed out (sign in again), 4 not found,
5 PerYourHealth or the network failed, 6 confirmation needed (`--force`).

## PerYourHealth's terms and automated access

www.peryourhealth.com links no terms of use; its sign-in page says the
system "is for the use of authorized users only" and that activity on it is
monitored. Optum's general Terms of Use forbid using "software or other
means to access, 'scrape,' 'crawl,' or 'spider'" its online services, and
copying or harvesting data from them; whether they cover this site, which
does not link them, is not stated. One Healthcare ID has terms of its own,
shown on its sign-in page. `peryourhealth` reads only what your own sign-in
shows you, one request at a time and spaced apart, and never writes
anything; you sign in yourself, in Chrome. Whether that suits you under
those terms is your call; Optum could change the site at any time.

## Privacy

Bills stay on your machine: `peryourhealth` prints them or pipes them to your
own records store, and contacts no server but www.peryourhealth.com (and
GitHub for `self-update`). The repository holds no personal data; its
fixtures are synthetic.

## License

MIT or Apache-2.0.
