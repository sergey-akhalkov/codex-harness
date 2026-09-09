# Native structured Codex inspection

Verified 2026-09-08 on Windows x64, Rust/Cargo 1.97.1 and native Codex 0.153.4.
This closes the real consumer acceptance in
[adoption task 4.3](../../openspec/changes/archive/2026-09-09-adopt-project-memory-and-native-workflows/tasks.md).
[migration task 7.2](../../openspec/changes/migrate-harness-to-rust/tasks.md)
remains open for the regression helper, linked invocation and global lifecycle.

## Implemented boundary

[The Rust helper](../../crates/codex-harness/src/structured.rs) and
[CLI](../../crates/codex-harness/src/bin/harness-inspect.rs) preserve the bounded
inspection flags, separate argv from prompt stdin, explicitly select read-only
sandboxing, and retain final JSON, events, process and oracle evidence separately.
The executable reads the skill's
[canonical schema](../../.agents/skills/structured-codex-run/assets/inspection.schema.json)
directly from source and passes that path to Codex. Native build identity covers
`harness-inspect.exe`; the schema remains live data. An installed candidate
resolves its source through its verified build receipt; an unregistered
development binary requires an explicit `--schema` source path.

Each child is admitted into the native Windows Job before execution, with
512 MiB committed memory, caller timeout and per-file output bounds. Polling may
overshoot an output limit; this is not a hard disk quota. The helper accepts only
natural exit zero, successful terminal events, fresh schema-valid output,
unchanged recorded inputs, no unresolved issues and an independent oracle.
Caller-specified paths and nonsecret route labels are retained in private local
evidence; the labels alone do not prove provider eligibility.

Parent review reproduced a false success when a trusted oracle changed a recorded
input after the initial fingerprint. The regression failed against the previous
implementation, then passed after checking inputs again after the oracle.
Counterexample: `%TEMP%/structured-codex-ceuLs5`. Output remains rejected even
when the oracle exits zero. Evidence directories inside the inspected target are
rejected; stderr classification reads at most 64 KiB; watcher cleanup precedes
propagation of process-wait errors.

## Deterministic checks

```text
cargo test -p harness-core --lib -p codex-harness --test structured --test native_build --offline --locked --jobs 1 -- --test-threads=1
```

The initial combined result was 6 structured, 6 build and 9 core tests passed. Structured cases cover
stdin/argv isolation, distinct process/auth/oracle/schema failures, stale run IDs,
timeouts, stdout/final-file bounds, retained partial evidence, fresh identities
and oracle mutation. Fixtures are compiled Rust. Specification review then
identified that embedding the source-owned schema violated direct data
consumption. That prototype was replaced with bounded live source reads. Seven
structured tests and nine core tests subsequently passed; the added case proves
live schema changes affect validation without recompilation, mid-run changes
are rejected, and unsupported schema keywords fail before child execution.
The helper retains path/hash evidence without a copied schema deployment.
The final combined migration verification remains open.

After that correction, a deterministic native consumer outside the checkout
resolved the already installed skill asset back to its source. It succeeded at
`%TEMP%/structured-codex-5OvjTu` with schema SHA-256
`4893dbda938242585d162d05c5ce2b656486b0a25effdd1e47d634c0ecd28bf6`
and no schema copy in the evidence directory. This used compiled fixtures and
made no model call; the separate actual Astra acceptance is recorded below.

## Real outside-checkout consumer

Preparation:
`%LOCALAPPDATA%/codex-harness-evidence/structured-native-9296dade93c14db0bad1f4c7c196fa79/`.
Execution: `%TEMP%/structured-codex-1P094e`, run ID
`42067346a6db26b1cc0f12126a5445fa`.

An owned Git repository at HEAD `568fa5ff7342578a105047240bf12190d550d9d8`
contained an invalid checksum. Its independent Rust oracle required the exact
source evidence, path and line; parent inspection separately confirmed the
description correctly identified the 64-character lowercase hexadecimal rule.
The model actually read `input.txt` through the native CLI.

The explicitly selected route was `gpt-6-astra`, low effort, provider `openai`,
existing ChatGPT through the unchanged OpenCodex endpoint. The owned home linked
existing authentication and the catalog without copying credential bodies.
The native executable SHA-256 was
`444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B`.
The first attempt rejected a reserved `model_providers.openai` override before
inference (`%TEMP%/structured-codex-1huC6x`); the corrected attempt used the live
configuration's `openai_base_url` contract. No provider or billing substitution.

The accepted process exited naturally with code 0 in 24.154 seconds, emitted
`turn.completed`, retained separate final JSON and events, and passed the
oracle. Peak Job memory was 130,310,144 bytes; active owned processes at receipt
were zero. Git status remained empty; input SHA-256 stayed
`21AD9A9D52A6761FD4740EE9C3A34FF1BD91626AA7933512808D0D60DBB01749`.
Recorded inputs were also unchanged after the oracle. This proves this bounded
native Astra consumer, not Grok schema enforcement or exhaustive auxiliary model
attribution. The existing global proxy was not restarted.

## Remaining delivery

The global skill still has its transitional Python entry point until the native
installation can resolve the helper through an owned immutable build. Native
candidate invocation is documented in its linked reference; no global executable
registration is claimed by this record. Regression parity and combined final
migration checks remain required.
