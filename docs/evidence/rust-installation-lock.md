# Native installation serialization

2026-09-09. [InstallationLock](../../crates/harness-core/src/installation_lock.rs)
uses the session-local named mutex of the transitional
[`Invoke-HarnessInstall`](../../tools/kit.psm1). The native
[`inventory` entry point](../../crates/codex-harness/src/main.rs) holds it through
inspection, so an active installer produces a bounded exit-2 error before source
discovery. Acquiring the lock does not create a directory or file.

The name retains the legacy absolute-path, trailing-separator and invariant
lowercase hash contract. Actual PowerShell 7.6.5 vectors cover ASCII, Cyrillic,
Greek, Turkish `İ`, Kelvin sign, emoji, mixed separators and `..` normalization.
This is lexical identity, not proof of ownership or serialization across other
Windows logon sessions or different filesystem aliases of the same home.

Windows [mutexes](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createmutexw)
are recursive and owned by a thread. The Rust guard rejects same-thread reentry,
cannot move between threads, and holds a non-inheritable handle.
[Release](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-releasemutex)
occurs on that thread. Abandonment is exposed to the caller; acquiring an
abandoned mutex does not certify that the previous installation is consistent.

Passed checks:

- Two core tests: exact legacy naming vectors, same-thread/case-alias refusal,
  competing thread, independent user and release/reacquisition without home files.
- Two real Rust process tests: contention, killed-owner abandonment and actual
  manager `inventory` rejection before source access. The fixture is explicitly
  ignored as a standalone test, has a 20-second fallback, creates no descendants,
  and its parent kills/reaps only the retained child handle.
- The actual PowerShell installer was invoked in Preview on an owned target while
  the Rust fixture held its mutex. It returned the expected busy error and neither
  target home was created. This did not run the global installer.
- Scoped Clippy passed; explicit Serena diagnostics reported no errors/warnings.

```text
cargo test -p harness-core --lib installation_lock --offline --locked --jobs 1 -- --nocapture
cargo test -p codex-harness --test installation_lock --offline --locked --jobs 1 -- --nocapture
cargo clippy -p harness-core -p codex-harness --lib --bin codex-harness --test installation_lock --offline --locked --jobs 1 -- -D warnings
```

Logs, source identities and the legacy interoperability receipt are under
`%LOCALAPPDATA%/codex-harness-evidence/installation-lock-6b858cc6a6804d9c91183fee07c114fd/`.
This supplies a lock consumer and migration compatibility evidence. Native
install/update/recover/disconnect still need to consume it and validate their
metadata before the installer migration can close.

## Separate connection and dependency owners

`InstallationLocks::acquire(user_home, dependency_user_home)` now acquires the
two legacy mutex identities in deterministic order, deduplicating aliases before
acquisition. Failure releases earlier guards while preserving the competing
owner. The guard remains bound to its acquiring thread. Its abandonment flag is
an observation requiring state inspection, not authority to recover or discard it.

The actual `inspect-installation --dependency-user-home ...` consumer now holds
both owners. A Rust child holding only the separate dependency owner caused a
bounded busy refusal before metadata access; after that child was reaped, the
same command returned the absent-installation result without creating any homes.
Four core tests and six CLI/process/legacy-state tests passed (one standalone
child fixture remains intentionally ignored). Clippy, Rustfmt and explicit Serena
diagnostics passed. Earlier single-owner and killed-owner checks were reused in
that run. No global files or services changed.

```text
cargo test -p harness-core --lib --offline --locked --jobs 1 --target-dir <owned-check-target> installation_lock -- --nocapture --test-threads=1
cargo test -p codex-harness --test installation_lock --test installation_state --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
```

Current logs and hashes are retained under
`%LOCALAPPDATA%/codex-harness-evidence/installation-owners-dcacb55d550f44638add4c56410444e3/`.
Full install/update/recovery orchestration remains unfinished.
