# Contributing

1. `make verify` must pass: fmt-check, clippy with warnings denied, the
   tests, the smoke test.
2. Fixtures are synthetic: a page shape becomes a file with the real page's
   structure and dummy values, never a real capture.
3. No personal data anywhere: code, docs, commit messages, pull requests,
   issues. `tests/no_personal_data.rs` enforces the shapes; review the rest.
4. A page change is observed on the signed-in site first and recorded in
   `docs/api.md`, traps included.
5. Drive the installed binary on synthetic data before calling a change
   done: `make install`, then every command and error path against a
   loopback server (AGENTS.md).
6. Squash merges only, with an approval and green CI.
