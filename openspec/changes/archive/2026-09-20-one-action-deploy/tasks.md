## 1. Reset and deploy core

- [x] 1.1 Add an owned-link reset to `core_install` that removes only
  recorded reparse-point links, preserves regular files, writes a receipt, and
  passes new unit tests for both cases using the existing fixtures.
- [x] 1.2 Add the `deploy` verb: absolute-source validation with its own
  error, default homes, candidate build or `--build`, `--reset`, core
  connect, installed-launcher verification and a single receipt; verify with
  unit tests and `cargo test -p codex-harness`.

## 2. Composition and docs

- [x] 2.1 Chain scoped component updates in `deploy --all` with per-component
  results in the receipt; verify the chain ordering and error reporting with
  unit tests.
- [x] 2.2 Document the one-action flow in `docs/installation.md` and the
  command mapping in `docs/rust-native.md`; verify local links and that the
  old multi-command path remains described for scoped use.
- [x] 2.3 Add the `harness-deploy` skill owning the delivery procedure
  (everyday deploy, blocked-installation repair, no out-of-band link edits)
  and verify the skill file is linked by a fresh core install.

## 3. Verification and delivery

- [x] 3.1 Run `cargo fmt`, workspace clippy `-D warnings` and the affected
  test suites; record results.
- [x] 3.2 Deliver this machine's installation through
  `codex-harness deploy --reset --all` and verify outside the checkout: the
  receipt reports the new build identity, installed `executor --help` serves
  usage, `--version` names the build source, and `update --preview` no longer
  reports blocked ownership.
  Receipt evidence: build `1b803110f184b7da`, 26 core links, runtime handoff
  passed, `executor_probe_ok`, `update --core-only --preview` clean. The chain
  stopped honestly at `token-workflow` with a pre-existing `os error 2`
  (recorded as an open finding; its `hooks.json` link was restored from the
  exact recorded target), so board and subscriptions were not re-run here.
