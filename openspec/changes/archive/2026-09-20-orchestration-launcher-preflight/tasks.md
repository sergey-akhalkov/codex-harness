## 1. Contract artifacts

- [x] 1.1 Add the launcher preflight and stale-launcher handling to the
  `team-lead` skill, and extend the `harness-core` skill-contract test to pin
  `executor --help`, the stale-build classification and the no-bypass rule;
  verify `cargo test --locked -p harness-core board_cli` passes.
- [x] 1.2 Document the live-skill versus immutable-build skew, its symptom,
  cause and update remedy in `docs/agent-delegation.md`, linking the
  installation guide; verify the delegation guide contains the preflight
  command and the no-bypass rule.

## 2. Manager diagnostics

- [x] 2.1 Change the unknown-command diagnostic to name the attempted command
  and the version-skew remedy, add a unit test for the wording, and verify
  `cargo test --locked -p codex-harness` passes.
- [x] 2.2 Report the adjacent immutable build's source identity in `--version`
  when present, keep cargo-built binaries on the plain version line, and verify
  both outputs from local binaries.

## 3. Verification and delivery

- [x] 3.1 Run `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets --locked --jobs 1 -- -D warnings` and
  `cargo test --workspace --locked --jobs 1 -- --test-threads=1`, and record
  the result of `harness-source-check` and `ownership-check` for this tree.
- [x] 3.2 Build a new immutable candidate from this checkout, deliver it, and
  verify outside this checkout: installed `codex-harness executor --help`
  prints usage, an unknown command names the command and remedy, `--version`
  names the build source identity, and the consumer-visible `team-lead` skill
  contains the preflight. The documented lifecycle update was blocked by
  pre-existing out-of-band installation damage (a recreated
  `harness/bin/codex-harness.exe` link no longer matches the recorded object
  identity, so update/disconnect refuse by design); delivery used a verified
  manual repoint of that one link to the new immutable build plus a refresh of
  the copied loop-guidance skill, and the clean lifecycle reset is handed to
  the owner as a script because bulk installation-file removal is outside this
  agent's tool policy.
