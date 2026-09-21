## Context

Codex rollout sessions are the authoritative local usage record. The local
corpus holds both the older `event_msg` token-count format and the newer
`token_usage_record` format (with `turn_token_usage` and
`thread_token_usage`), and `session_meta`/`turn_context` carry the full base
and developer instruction texts, model, reasoning effort and project cwd.
`codex-harness delegation-usage` already parses these files for delegation
accounting in `crates/codex-harness/src/delegation_usage*.rs` (~50 KB with
tests through the real CLI), including response deduplication, privacy
hashing and `--private-sources`. Nothing today scans the whole sessions
directory for optimization analysis, and `harness-core` is the existing
shared owner of native harness primitives.

## Goals / Non-Goals

**Goals:**

- One rollout event reader shared by delegation accounting and the new
  analyzer, with the existing `delegation-usage` contract unchanged.
- Measured-first analysis: report and findings distinguish recorded values
  from inference and estimation at the field level.
- A closed optimization loop: report, findings with owners and validation
  plans, baseline save/diff.

**Non-Goals:**

- Currency costs, quota percentages or allowance attribution.
- A TUI dashboard, provider plugins beyond Codex rollouts, network access, or
  auto-applied fixes.
- A persistent index or cache; streaming scans are the baseline design.
- Changing `token-efficient-agent-workflow` (runtime discipline) or
  `subscription-efficiency` (delegation accounting) requirements.

## Decisions

### Separate crate with a thin binary

`crates/token-audit` joins the workspace as a library plus a `token-audit`
binary, following the `windows-disk-reclaim` precedent. A `codex-harness`
wrapper subcommand can be added later if installed-path ergonomics need it,
but the analysis tool starts with its own small blast radius and test
surface. Alternative rejected: implementing as a `codex-harness`
subcommand, which couples analysis iteration to the launcher binary and its
lifecycle checks.

### Shared reader in `harness-core`

Extract a `rollout_reader` module into `harness-core` that owns the tolerant
event vocabulary: recognized event types, both usage formats, response
identity deduplication, turn association, instruction byte capture and
coverage warnings. `delegation_usage` keeps its parent/child accounting and
markdown output but consumes the shared reader; `token-audit` consumes the
same reader. Alternatives rejected: a second parser (format-drift risk),
shelling out to `delegation-usage` (wrong output shape and a process
boundary), and a separate reader crate (`harness-core` is already the shared
primitives owner).

### Metric definitions stay explicit

- Context repayment multiplier per session: summed per-turn input tokens
  divided by the final turn's input tokens; both terms are recorded values.
  It measures context replay volume, not price, and is reported alongside
  cache efficiency rather than replacing it.
- Instruction floor: byte sizes of recorded base instructions and per-turn
  developer instructions. Bytes only; no token conversion is invented.
- Tool output mass: byte totals per tool name from recorded tool outputs.
  Any statement about tokens consumed by a tool is labeled inferred.

### Findings are data, not advice text

Findings emit a versioned JSON object: id, basis
(`measured`/`inferred`/`estimated`), `mass_tokens`, evidence locators
(session and turn ids, hashed project), owner (path to an existing record or
skill), and a validation plan (`method`, `metric`, `command`). Detectors are
small modules behind this contract, each with synthetic fixtures. Copy-paste
instruction text is deliberately absent; remediation lives in owning records.

### Baselines as local aggregate snapshots

`baseline save` writes one small JSON aggregate per audit under
`CODEX_HOME/harness/token-audit/baselines/` with a `latest` pointer; `diff`
compares a fresh report to a named or latest baseline. Snapshots contain
aggregate metrics and hashed identities only. No pruning in the first
release; retention can be added with a real need.

### Streaming scans without an index

The reader streams rollout files line by line and deserializes only the
event envelope plus recognized payloads; unknown lines increment coverage
counters. The measured local corpus (~850 MB across 212 files) is small
enough for repeated full scans in Rust; `--days N` bounds interactive runs.
A cache or index is added only if measured scan latency becomes a concrete
friction.

### Skill as consumer, delivered by the existing lifecycle

`.agents/skills/tokenomics/SKILL.md` is a diagnostic skill: it invokes the
analyzer, maps findings to existing owners, and records decisions in
existing records. Its description is worded to trigger on usage analysis,
not on runtime efficiency work, keeping it disjoint from
`token-efficient-workflow`. Delivery uses the kit's existing skill
installation path, verified against an installed consumer outside the
checkout.

## Risks / Trade-offs

- [Rollout format drift across CLI versions] → tolerant reader with
  per-format coverage counts; both currently observed formats covered by
  fixtures from day one; unknown events never fail a scan silently or
  loudly.
- [Extraction regresses delegation accounting] → extraction lands as its own
  task before analyzer work; the existing delegation-usage CLI tests must
  pass unchanged.
- [Token attribution limits frustrate users] → tool-level attribution is
  labeled inferred and findings default to `measured` basis, so expectations
  are set by the output contract itself.
- [Session corpus growth slows audits] → streaming and `--days` bounds
  first; index/cache only after a measured latency complaint.
- [Skill adds a fixed prompt cost to every session] → keep the description
  short and diagnostic-specific; measure the instruction-floor delta in the
  analyzer itself.

## Migration Plan

1. Extract the shared reader into `harness-core` with delegation-usage tests
   green (behavior-preserving refactor; revert is a single commit).
2. Add `token-audit` report, findings and baseline commands behind their own
   tests and fixtures; no existing command changes.
3. Add the skill and installation verification; update executable-ownership
   accounting and owning docs.

No data migration is required; baselines are new local state, and removing
the crate leaves existing sessions and delegation accounting untouched.

## Open Questions

- Whether the `token-audit` binary eventually needs a `codex-harness`
  wrapper subcommand for installed ergonomics; deferrable until the
  installed-consumer verification shows friction.
