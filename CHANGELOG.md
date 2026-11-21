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
