# Native Rust implementation

The [migration specification](../openspec/changes/archive/2026-09-17-migrate-harness-to-rust/proposal.md)
requires Rust for all maintained harness-owned executable code, including tests
and skill helpers. This is a rule for pack development; it does not change the
languages of projects using the kit. External Codex, Serena, Nuphus and
language servers retain their own implementations and supported runtimes.
CodeGraph, Codebase Memory and Graphify are retired and carry no first-party
code.

The native lifecycle is the live global installation: the manager, launcher,
diagnostic alias, MCP servers and RTK adapter are Rust. The transitional
PowerShell and Python lifecycles are retired from the repository; machine-local
rollback copies are not maintained source. Evidence and remaining acceptance
live in [Rust migration](evidence/rust-migration.md).

## Supported command mapping

The retired script interface and its native equivalent. Selectors, preview
(`--preview` for `-WhatIf`), homes and source arguments are preserved.

| Retiring script interface | Native equivalent |
| --- | --- |
| `install.ps1 -Mode Install` / `-Mode Update` | `codex-harness install` / `update`, with `--core-only`, `--code-tools-only`, `--subscriptions-only`, `--token-workflow-only` or `--board-only` |
| `install.ps1 -Mode Check` (`-Diagnose`) / `codex-harness-check.ps1` | `codex-harness check` / `check --diagnose` / `codex-harness diagnose` |
| `install.ps1 -Mode Disconnect` / `-Mode Recover` | `codex-harness disconnect` / `recover` |
| `install.ps1 -Mode ConfigureRestart -SubscriptionsOnly` | `codex-harness configure-restart --subscriptions-only` |
| `tools/mcp.ps1 <server>` | `codex-harness mcp serena` / `nuphus` |
| `tools/hook.ps1` | Retired no-op compatibility entry; the accepted RTK exception runs native `harness-rtk.exe` |
| `tools/delegation-usage.py` | `codex-harness delegation-usage` |
| First-party usage analyzer | `token-audit report` / `findings` / `detail` / `baseline save|diff` (crate `crates/token-audit`, operated by the `tokenomics` skill) |
| Instruction/source audit | Installed `harness-source-check --root CHECKOUT` |
| Verification evidence | `harness-observe --scope TEXT --input FILE ... -- EXE ARGS` |
| Feedback mechanics | `codex-harness feedback record|list|ledger|triage|candidates|promote` |
| `tools/outcome_*.py` helpers | `codex-harness outcome-prepare` / `outcome-oracle` / `outcome-discover` / `outcome-arm` / `outcome-run` / `outcome-report` |
| Subscription login / restore scripts | `codex-harness subscription-login xai` / `zai` (native restore in the service host) |
| Skill evolution helpers | `codex-harness skills isolate` / `usage` / `publish` / `identity` |

## Prerequisites and checks

Windows x64, Rust MSVC toolchain (minimum Rust 1.98, currently verified with
1.98.1), Cargo, the MSVC C++ linker and Windows SDK. Workspace dependencies
are locked in the root
`Cargo.lock`. The RTK adapter is a workspace member with no independent
lockfile. Use the existing toolchain; Check does not install packages.

```powershell
codex-harness heavy -- cargo build --workspace --locked --jobs 1
cargo fmt --all -- --check
codex-harness heavy -- cargo clippy --workspace --all-targets --locked --jobs 1 -- -D warnings
codex-harness heavy -- cargo test --workspace --locked --jobs 1 -- --test-threads=1
cargo run -p codex-harness --bin harness-source-check -- --root .
```

The executable ownership check compares the working tree against
[executable-ownership.json](evidence/executable-ownership.json). Unclassified
foreign-language executables (tracked or untracked), embedded or generated
foreign programs in maintained Rust source, first-party paths relabeled as
third-party or inert data without genuine consumer evidence, and stale
inventory entries fail the check. Remaining classified first-party legacy
paths stay explicit open findings owned by their migration task, so the
command exits nonzero until that removal finishes:

```powershell
cargo run -p codex-harness --bin codex-harness -- ownership-check --source .
```

Optional `--private-terms <external-file>` supplies local audit terms without
persisting or echoing them. The checker inspects current Git-listed tracked and
new existing files, tracked caches, machine-home paths in docs/shared
configuration, shared `[projects.*]` records and inline Markdown local
targets/anchors, skipping fenced examples, templates and URLs. It does not
inspect Git history.

Core delivery also installs `harness-source-check` on PATH. From any directory,
run `harness-source-check --root <kit-checkout>` to audit that checkout without
compiling or starting a model. The checker enforces the portable principles'
24 KiB byte limit. For a kit root identified by `global/kit.json`, it also
checks token-audit's declared documentation owners. These mechanical checks
do not certify semantic completeness or inspect Git history.

Current-path baselines for the Rust migration are captured by
`cargo test --locked -p codex-harness --test migration_baseline --jobs 1 -- --test-threads=1 --nocapture`
after `cargo build --locked -p harness-rtk --jobs 1`. The comparison method
and noise tolerance live in [Rust migration](evidence/rust-migration.md).
Detailed receipts stay in the printed private TEMP root.

The current requirement-to-check map lives in
[rust-requirement-checks.json](evidence/rust-requirement-checks.json).

Run heavy builds and checks through the [heavy-command budget](#heavy-command-budget).
Those admitted commands also join the [shared CPU allowance](#shared-agent-cpu-allowance).
`--jobs 1` also bounds compiler concurrency within the admitted command.
A failed compilation retains its log and does not alter the active
installation. Windows can keep a running test executable locked after Cargo
releases its build lock; use `--target-dir <owned-verification-directory>`
when a retained test binary could occupy the default target. That is a Cargo
output choice, not an ambient `CARGO_TARGET_DIR` override for native build
identity. Custom compiler wrappers and ambient Rust build overrides are
rejected before creating build state.

## Heavy-command budget

`codex-harness heavy -- PROGRAM ARGS` runs one batch command and its descendants
under the account heavy-command slot set. The default is 2 concurrent trees.
Callers from different projects or `CODEX_HOME` directories share that bound;
only a caller past it waits. Source reads, edits and model conversations stay
outside the slots. Native managed builds use the same admission path. Route
heavyweight commands from free-text executor assignments through this entry
point too; structured briefs include it already.

```powershell
codex-harness heavy --help
codex-harness heavy budget --json
codex-harness heavy -- cargo test --locked --jobs 1 -- --test-threads=1
```

A missing `max_concurrent_trees` is 2. A missing
`aggregate_memory_limit_bytes` equals the effective per-tree limit, 8 GiB when
that limit is also the default. The aggregate is one account envelope,
`JOB_OBJECT_LIMIT_JOB_MEMORY`, covering every admitted tree; each tree keeps
its own containment Job. The deadline still defaults to 30 minutes and the
queue wait to one hour. There is no per-operation CPU default. Admitted
commands use the unchanged shared account ceiling, 75% of host CPU, not 75%
per slot, in [Shared agent CPU allowance](#shared-agent-cpu-allowance). The
top-level `codex-harness --help` summary names the slot set and does not list
`--uncapped`; `heavy --help` does.

Policy field `max_concurrent_trees` set to 1 is the serialized rollback: the
exclusive legacy `heavy-command.lock` and no slot file. A larger count holds a
shared lock on that same file for the admission lifetime, so a legacy exclusive
lock still blocks current callers while slot files are free. Slot files alone
do not admit. `heavy budget` reports the slot count and aggregate limit; it has
no flag that changes them. Reads do not rewrite a policy that omits the new
fields.

`heavy budget` prints effective per-tree memory, aggregate memory, slot count,
deadline, queue wait, CPU policy, field sources and the local policy path.
With no heavy-command policy file, `cpu_percent` is absent, `shared_cpu_percent`
is 75 and `cpu_policy` says there is no per-operation CPU limit. Inspection
creates no account state and does not start work. Explicit
`--memory-bytes`, `--cpu-percent`, `--deadline-seconds` and
`--queue-wait-seconds` update that machine policy; `--preview` shows the
proposed change and does not write it. `--cpu-percent shared` removes a
per-operation CPU limit. A lower `--cpu-percent P` is a deliberate ceiling in
percent of host CPU, translated against the kernel-verified parent rate,
rounding down, and reported with its effective host-relative value. A recorded
value of 50 cannot be distinguished from the retired batch default: it is
preserved, `legacy_default_cpu_percent` is true, and it is not silently
rewritten.

The heavy-command policy stays at
`$env:LOCALAPPDATA\coding-agents-harness\heavy-command\budget.json`, outside
the kit and consuming repositories. It is not the shared CPU policy record.
`--account DIRECTORY` selects an isolated heavy-command account for owned
tests; normal callers retain the default shared account.

A waiting caller prints one bounded queue line naming the busy-slot count, the
total slots and available holder descriptions. When no holder record is
available, that line says the legacy lock is held. Child output is streamed
and the child's exit code is preserved. Deadline or queue expiry returns 124,
memory exhaustion 125, incomplete cleanup 126, start failure 127 and
interruption 130. Normal completion and termination release the slot. Nested
heavy commands inherit the outer admission, take no second slot, and verify
membership in the holder's Job without a second CPU cap. Commands launched
outside this entry point are not in the heavy queue. Descendants of an admitted
agent route still share the account CPU allowance; the queue does not cover
arbitrary processes started outside those routes.

```powershell
codex-harness heavy --uncapped -- cargo test --locked --jobs 1 -- --test-threads=1
```

`--uncapped` is an exception for that one command, not the normal test path.
It is not saved. Other sessions and shared services keep their allowance, and
the next command without it is capped. Combined host agent load can exceed the
shared ceiling while the exception runs.

## Shared agent CPU allowance

On Windows, ordinary installed routes share one account CPU allowance: 75% of
total host CPU, not 75% per session. The routes are the Codex bootstrap,
task-control, executor and resume session hosts, `codex-harness heavy`, and
kit-owned shared MCP services and backends. Direct native descendants are
covered from the start of execution. An unrelated terminal tab or desktop
application is not enrolled. Shared services stay in the allowance even when an
uncapped client calls them. Executor, resume and task-control have no separate
uncapped flag; they join automatically.

The machine-local record is
`$env:LOCALAPPDATA\coding-agents-harness\cpu-budget\shared-cpu-policy.json`.
It is not `cpu-budget.json`. Core install creates it only when absent. See
[Shared CPU policy](installation.md#shared-cpu-policy) for creation, preservation,
safe restart and disconnect. `CODEX_HARNESS_CPU_PERCENT` overrides the ceiling
for the launch that reads it when the value is a percentage from 0.01 through
100. It is not written into `shared-cpu-policy.json`. If the account job does
not exist yet, that admission establishes the job at the requested rate; the
kernel ownership record is `cpu-budget.json`, not the policy file. An existing
job at another rate is preserved and not modified, and that launch is not
admitted. Unset the variable to request the policy ceiling, or 75% when the
policy record is absent. A non-numeric or out-of-range value does not fall
through to the policy record. That failure, an unusable record or a failed
admission warns on stderr and starts the requested payload once outside the
verified group. It does not substitute another ceiling, and it is not the
uncapped notice. The session warning names the requested
ceiling, failed stage, cause, scope and recovery. A heavy command continues
once under its queue, memory and deadline contracts without a verified shared
ceiling. The next normal launch still attempts the default. Other sessions
keep their caps. If the Codex bootstrap cannot open account storage, its
recovery names `CODEX_HARNESS_CPU_ACCOUNT` or restoring `LOCALAPPDATA`. That
variable selects the account directory; it is not a second ceiling.

Request one uncapped session or command explicitly. The session selector is
consumed in the harness-option prefix, including beside `--harness-effort`,
and is not forwarded to the payload:

```powershell
codex --harness-cpu uncapped
codex --harness-cpu=uncapped
codex-harness heavy --uncapped -- PROGRAM ARGS
```

`uncapped` is the only accepted mode, case-insensitive. Any other value is a
usage error and does not start the payload. The exception is not saved, does
not raise other sessions or shared services, and the following invocation
without it is capped. Combined host agent load can exceed 75% while it runs.
A copied marker that leaves the process inside the shared group is not a
successful exception.

Coverage inspection is core check, not `check --diagnose` and not a separate
status command:

```powershell
codex-harness check --core-only --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
```

`cpu_budget` reports configured ceiling, kernel readback, covered and uncovered
routes, explicit exceptions, degraded starts, unknown members and
`restart_boundary`. `cpu_policy.escape_hatch` is `CODEX_HARNESS_CPU_PERCENT`.
`measured_consumption` is `not-sampled`. Do not treat configuration readback as
measured enforcement. Inspection does not write policy, create a job, sample
CPU or call a model. What incomplete activation and the disconnect notice mean
is in [installation](installation.md#shared-cpu-policy). The decision is in
[project decisions](project-decisions.md#shared-agent-cpu-budget).

## Structured executor assignments

`codex-harness executor spawn`, `resume` and `restart` accept `--assignment FILE` or
`--exec PROMPT`, and `--exec -` reads the free-text assignment from piped
standard input instead of the command line. Keep the file local or in the
owning project; do not put private consumer details into this pack. Example
schema:

```json
{
  "schema": 1,
  "objective": "Implement board item sample-17: validate parser input",
  "inputs": ["Cargo.toml", "src/parser.rs"],
  "outputs": ["src/parser.rs", "tests/parser.rs"],
  "invariants": ["Preserve the public result type"],
  "acceptance": ["Run the parser tests and report actual results"],
  "consumer": "the lead session for sample-17",
  "escalate": ["any change to the public result type"]
}
```

`consumer` and `escalate` are optional, and both are additive. Omission of
`consumer` names the dispatching lead as the result consumer. The rendered
brief always carries the standing escalation boundaries - a change to the
agreed outcome or scope, a material architecture or design change, missing
authority or access, and a concrete dependency the executor cannot obtain -
so a declared `escalate` item adds a task-specific trigger instead of
replacing them, and an omitted field adds nothing. Nothing else escalates:
ordinary implementation errors (syntax, incorrect API names, failing checks)
belong to the executor, which investigates, corrects and re-runs them.

The brief also carries the executor's own work cycle - read the declared
inputs, investigate the current source and callers before editing, implement
the owned outcome, run the applicable checks, correct local errors and repeat
- and the compact result the consumer expects back: done and remaining work,
the checkout and base worked from, the exact files touched, how each acceptance
item was verified with the checks actually run, limitations, the decision
needed from the consumer and where the full detail lives. Source bodies are
supplied only as an explicit fallback when reading is unavailable.

Paths are relative to the allocated checkout. Inputs must exist as files;
outputs may be new. Absolute paths, traversal and links escaping the checkout
are rejected before a model starts. The generated brief names the real checkout
and full committed base, preserving the caller's objective and acceptance.
Schema/path checks cannot establish semantic completeness or execution success.
Resume retains partial work. Free text remains supported.

For an existing pool slot, `codex-harness executor assignment --source CHECKOUT
--slot N --base REV --assignment FILE` validates/renders without allocating,
resetting, writing or launching a conversation. Normal dispatch still uses the
installed profile. Omitting `--mode` selects the native TUI; explicit
`--mode exec` is the inline presentation on the same lifecycle.

The observed lifecycle of dispatched sessions is a native command, not a log
search: `codex-harness executor watch` (same optional address fields, or
`--receipt FILE` with an absolute path) blocks on the receipt's recorded
lifecycle and prints bounded review data. Exit 3 means action required: the
live run holds an unanswered reply request whose one-command reply reference
the result carries - a state to answer, never a completion, an output defect
or a resume trigger.
`codex-harness executor run --file RECEIPT` is the tab or console host: it
attaches the native TUI, inline when spawn selected exec, and records that
lifecycle rather than launching an unobserved `codex exec`. States, exit codes,
automatic closure and exact-session resume are in
[agent delegation](agent-delegation.md#observed-executor-lifecycle).

A live run is addressed and controlled through its recorded identity; address
fields are optional and resolve only from harness records:

```powershell
Get-Content correction.txt | codex-harness executor message
codex-harness executor message --text 'short correction'
codex-harness executor message --reply-to MESSAGE_ID
codex-harness executor stop
codex-harness lead message [--notify]
```

Message content is piped standard input (`--text -` selects it), a short
`--text`, or explicit UTF-8 `--file`; no size has to be known and no file
created, and the payload arrives verbatim - line breaks kept, no shell
evaluation - in the run's own conversation at the nearest supported point,
even mid tool call, without interrupting it. Above the 256 KiB inline
bound either direction - including a `--reply-to` reply - spills
automatically: the command persists the payload under
`CODEX_HOME/harness/messages/` and sends a pointer envelope naming the exact
size and absolute path for the recipient's own tools. Receipts and watch report
`delivery: spill` with `payloadPath` and `payloadBytes`, never inline delivery,
and above the 8 MiB ceiling the refusal names the limit and sends nothing.
Inline results stay `queued`, `delivered` and `error`: a local write is never
model delivery, and an indeterminate retry cannot deliver the same text twice.

Explicit address fields (`--source`, `--codex-home`, `--slot`, `--owner`,
optional `--session`) stay accepted for disambiguation and scripting and are
verified exactly like resolved values.
`--codex-home` defaults to `CODEX_HOME` or the recorded installation default,
`--source` to the installation record's checkout, and slot, owner and session
resolve from the recorded live pool lease and receipts when exactly one run
matches; several live runs refuse with a bounded listing naming `--slot`, a
stale binding refuses the identity mismatch, and resolution reads only harness
records - never the working directory, process names, window titles or most
recent session. Lifecycle commands keep requiring explicit paths. A completed,
stopped or unavailable run gets its state and exact-session resume remedy; no
recorded control endpoint means `message` is unsupported with the same remedy,
while `stop` still works through the recorded host identity.

`stop` urgently ends one exact run - native interruption where the backend
provides it, else bounded termination of the recorded host's process tree,
verified by recorded identity and never by process id, program name or window
title - closing exactly that run's tab while the lead's terminal and
neighboring tabs stay usable, and records `stopped`, `already-completed`,
`partial` or `error` with an unknown exit code kept unknown and pending
messages marked undelivered; a partial stop names the surviving process, cause
and next action. Stop keeps files, the checkout, the slot and partial work (no
reset, clean, release or completion claim); a repeated stop reports the current
state, and a stop racing natural completion reports which outcome won.

`lead message` is the executor's direction and its only channel: a
literal payload from piped input, `--text` or `--file` goes to the originating
lead recorded for its own live run, so the caller supplies no address, and
`--notify` marks an exceptional notice that requests no reply. The default kind
requests one; the lead answers with
`executor message --reply-to MESSAGE_ID`, which resolves that request instead
of an address and continues the same conversation in place, and an unanswered
request keeps the run, its session, slot and worktree live (`executor watch`
exit 3) without a resume. Selection rules live in
[agent delegation](agent-delegation.md#steering-and-stopping-executors).

```powershell
cargo test --locked -p codex-harness --test executor_observation --jobs 1 -- --test-threads=1
cargo test --locked -p codex-harness --test executor_spawn --jobs 1 -- --test-threads=1
```

`executor_observation` drives the owned Rust event fixture
(`crates/codex-harness/src/bin/harness-executor-fixture.rs`) through the real
host, receipt, watch, resume and release paths - including setup failure before
any model process, observer-failure termination of the owned launcher tree,
containment when the host itself is killed, changed-file comparison against the
recorded base and concurrent receipt writers - and makes no model request. The
ignored `installed_native_exec_json_event_shape_stays_observed` check in that
target is the opt-in installed-CLI shape check: it requires
`HARNESS_CONTROL_CODEX_EXE` pointing at the native Codex executable and runs it
against the existing synthetic Responses provider, so it validates observed
event behavior without a subscription or paid model call.

Cache-loss acceptance also has two opt-in native checks:
`native_control_cache_loss_stops_a_fresh_session_and_preserves_work` in
`executor_control` and `installed_native_cache_loss_stops_before_more_requests`
in `executor_observation`. They use local canned Responses with a warmed cache
followed by three large misses, require `HARNESS_CONTROL_CODEX_EXE` and
`HARNESS_ACCEPTANCE_POWERSHELL` (the owner's existing PowerShell 7), and verify
termination with bounded request counts and preserved work. Set
`HARNESS_OBSERVATION_MANAGER_EXE` to the installed manager to exercise that
binary; the compiled test targets can run from outside the source checkout.
No real provider, credentials or subscription is used by these checks.

## Bounded token reports

Use `token-audit report --format text` and `token-audit findings --format text`
for ranked summaries. They state omitted counts and retain complete coverage,
warnings and limitations. Full same-scan JSON is retained locally (newest 20
files per kind); the summary prints its locator. `token-audit detail --report
PATH --session ID` or `detail --findings PATH --finding ID` reads just the
requested record without rescanning sessions. Missing/evicted evidence is an
error. `--format json` preserves the complete contract; baselines are unchanged.
The [audit owner](memory/token-audit.md) describes interpretation and limits.

## Verification capture

The optional `harness-observe --scope TEXT --input ABSOLUTE-FILE ... --
ABSOLUTE-EXE ARGS` path records executable and declared input identities before
and after execution, cwd, command arguments and Git identity when available.
Select source, command-definition and build inputs relevant to the check.
Evidence remains in the observer's existing local case directory. Missing
inputs fail before launch; failure, timeout, changed inputs and unavailable
identity remain distinct. No automatic acceptance, cache reuse or claim about
unlisted files/external state is made. See the installed verification skill's
[command records](../.agents/skills/project-verification/references/command-records.md)
for ordinary use and interpretation.

## Feedback operations

`codex-harness feedback` uses the consumer's existing `bd` board. `record`
creates a bounded observation; `list`, `ledger` and `candidates` inspect it.
`triage --decisions FILE` applies caller-selected grouping/kinds, counts distinct
reporter/episode votes, excludes diagnostic votes and enforces the configured
batch limit. Every command identifies its limits; an explicit `--source KIT`
overrides the installed kit's configuration.

`promote` records an explicit backlog, OpenSpec or sanitized kit-backlog route.
The OpenSpec route validates an existing change created through the project's
OpenSpec workflow (`--openspec-change NAME`, default `feedback-<item-id>`);
it preserves planning artifacts. Consequence overrides require a stated reason.
Partial operations report the applied prefix and failing action with nonzero
status, and retries preserve counted votes and promotion history. Grouping,
consequence, authorization and acceptance remain agent decisions. Command
examples and routing rules live in
[board workflow](../.agents/skills/board-workflow/SKILL.md).

## Native task-control contract

The [subscription orchestration change](../openspec/changes/archive/2026-09-20-orchestrate-subscription-agents/tasks.md)
is under development. Its bounded Rust WebSocket client has an opt-in native
contract check; global activation and quota handoff remain unfinished.
Supply the intended native CLI executable explicitly, then run from this checkout:

```powershell
# Set HARNESS_CONTROL_CODEX_EXE to the native Codex executable being qualified.
cargo test --locked -p codex-harness --test task_control_contract native_two_clients_reconnect_tool_result_and_tui --jobs 1 -- --ignored --nocapture --test-threads=1
```

The ordinary native-entry acceptance is a separate opt-in test:

```powershell
cargo build --release --locked --workspace --bins --jobs 1
cargo test --release --locked -p codex-harness --test task_control_launch ordinary_launcher_starts_control_and_delivers_tool_result --jobs 1 -- --ignored --exact --nocapture
```

It uses actual Cargo binaries in an owned fixture registration and the same
explicit native CLI input. The fixture enables `task_control`; existing
registrations default to disabled and the installer does not activate this
unfinished controller yet. The test requires automatic service startup, exact
model binding, a single tool effect, visible final output, a controller checkpoint
containing that result and verified service exit after the native TUI quits.
Fixture registration is not evidence for the unfinished global install lifecycle.
The native-entry scenario passed on Codex 0.154.0 on 2026-09-12, including the
checkpoint and owned-process exit assertions. Its printed private root retains
binary hashes, provider requests and terminal evidence. A local synthetic provider
does not establish subscribed model availability or quota-handoff behavior.

This check uses private owned state, Windows Jobs, a token-protected loopback
app-server, a local canned Responses provider and ConPTY. It makes no live model
requests. It captures the CLI version/hash, generated experimental protocol
schema, request/event evidence and terminal output in the printed private root.
It exercises a native child assignment, one actual tool mutation, two-client
event delivery, reconnect while the child remains active, final-result retrieval
and visibility in the native TUI. The mutation must occur exactly once.

Verified on Codex 0.154.0: the app-server exposes child identities through
`subAgentActivity.agentThreadId`, with `parentThreadId` and `sessionId` available
on resume. A child can be observed/resumed but reports `canAcceptDirectInput=false`.
The canned native collaboration call uses namespace `collaboration`, an underscore
task name and `encrypted_function_args: []` to identify its known plaintext brief.
This is a fixture protocol contract, not permission to reinterpret opaque history.
The test pauses its synthetic native goal and bounds provider requests: an active
native goal can generate further turns after a canned final response. The future
controller must coordinate with that scheduler rather than dispatch twice.

The ordinary TUI also creates ephemeral system threads for generated titles and
uses paginated history. These threads are not user task owners and reject full
history reads. Subscribe to a user thread after its first active turn establishes
a resumable rollout; retain completed-item events rather than relying only on an
early history snapshot for the final result. The owned provider supplies a title
response separately from the tool-using assignment.

The separate `native_visible_chats_before_model_dispatch` test in that target
requires `HARNESS_CONTROL_VISUAL_ACCEPTANCE=1` and opens three owned native windows.
Its private `views-ready.json` identifies the exact threads; scoped Nuphus
observation supplies `views-observed.json` with those `threads` and
`simultaneouslyVisible=true`, then `visible-result-observed.json` with
`finalVisible=true` after the native final marker is seen. Observation waits are
bounded, and owned Jobs clean the windows even after assertion failure. These
receipts record desktop evidence; the probe uses only canned local responses.

The native path can open empty conversations with zero provider requests, name
them through `thread/name/set`, then receive controller-initiated tool work and
final output in their existing windows. Naming before dispatch avoided automatic
title requests in the owned scenario. The `native_named_empty_thread_can_be_attached`
test separately verifies controller-created, named empty threads can be resumed
before a model request, then exercises native TUI attachment and a synthetic
tool result. The controller must first resume the named empty thread by ID:
attaching the TUI directly by its rollout path otherwise fails with missing
source history on the tested CLI. This preparation makes no model request.
Remote resume rejects CLI permission overrides even when they match
the thread. For a new managed thread those options remain on the backend and
are omitted from the attaching TUI; prompt, model, effort and presentation
options remain. An explicit resume that requests changed permissions keeps the
ordinary native path, with a diagnostic, until equivalent managed behavior is
verified. Observers of TUI-created threads must subscribe after
the first active turn; global thread-start events alone do not subscribe them
to final turn events. The new visible-console option retains Job ownership
and gives the new window its own standard devices.

The native-entry test opens an owned conversation window, then checks that
window against the release wrapper (`build/codex.exe`), writes
`native-entry-result-observed.json` with `finalVisible=true`, and sends `/quit`.
Do not point `Snapshot::is_visible` at the upstream Codex binary: the TUI process
is the harness wrapper. Spawned children get their own named window (`Executor N`
or `Helper N`) before the fixture answers their first model request. View-loss
restore still uses scoped desktop receipts. Global delivery remains unfinished.

A frontend that exits early records its native exit code in private
`client-closed.json`; the entry-point test reports that exit immediately instead
of waiting for a model result from a closed conversation.

The separate `ordinary_launcher_suspends_and_recovers_visible_conversation`
entry-point test pauses a command after its first file effect. At its private
`native-entry-partial-ready.json`, minimize the identified owned window with
scoped Nuphus input. Wait for `native-entry-suspended-ready.json`, which requires
native interruption, preserved file content and no second provider request.
Restore that same window, verify its actual state and supply
`native-entry-view-restored.json` while the original tool is still running. The
test checks that restoration alone does not issue another provider request,
then releases the tool and awaits its native completion before continuation.
Then use the ordinary final-observation and quit steps above. The restoration
and final desktop gates each allow 90 seconds;
run consecutive interaction steps together rather than leaving a gate pending
while investigating other work. This scenario passed with synthetic Responses:
one file effect, visible continuation and final result, exactly three requests,
native exit zero and owned controller cleanup. The ordinary entry scenario also
passed on the same build. These checks cover one conversation; simultaneous
active workers, frontend death/recreation and quota handoff remain unfinished.

The controller watches the retained frontend process identity and actual window,
including minimization; an old `view-state.json` is not readiness evidence.
Native turn interruption does not prove a running tool has stopped. The observer
retains active tool items until their completion events and waits for them before
resuming the interrupted conversation, including after its window is restored.
Private checkpoint reads/writes retain transactional guards and retry only
Windows sharing/lock conflicts for at most 500 ms. Other errors remain failures,
and this retry never repeats a model or tool request. The focused checkpoint
tests cover a short reader and a persistent lock preserving the previous state.

The separate `ordinary_launcher_records_quota_refusal_without_another_gpt_request`
fixture returns one structured synthetic usage-limit refusal before the lead can
issue a fallback instruction. The controller records the correlated terminal
error in private `failures.json`, retaining the native error and observation time.
Its classifier distinguishes a usage limit, throttling, a session budget,
connection failure, interrupted output and exhausted retries; unrecognized or
ambiguous evidence stays unknown. A generic HTTP 429 or quota wording does not
establish exhausted subscription capacity or its reset window. Observe the
failure in the native window and use the ordinary final receipt and `/quit`
steps. A native `systemError` can settle after its latest failed turn is observed
and active operations have finished; an old failure cannot settle a newer turn.
The native refusal and ordinary tool-result cases passed on the corrected build,
including visible output and owned controller exit. The following separate check
covers the first automatic successor dispatch.

`ordinary_launcher_hands_quota_refusal_to_visible_zai_lead` exercises the integrated
successor path with synthetic Responses and an explicit local
`HARNESS_CONTROL_MODEL_CATALOG` input. The fixture copies model metadata only;
it neither imports credentials nor contacts subscriptions. It requires a fresh
Z.AI thread, unchanged initial permission binding, preserved visible task context
without opaque reasoning, and both native views at the first successor request.
The wrapper waits for the native TUI's named-thread caption before reporting the
new view ready. At `native-entry-result-ready.json`, inspect `view` for the Z.AI
result and `previousView` for the original failure/history, then supply the final
observation receipt and `/quit` in both windows. This native check passed: one GPT
refusal, two `zai/glm-5.3` requests at advertised `high` effort, one tool effect
through the catalog's Code Mode interface, successful tool output consumed by the
successor, visible final result, retained previous chat and natural controller exit. It used
owned synthetic Responses and the explicit local model metadata input, not live
subscription capacity. Global activation, concurrent executors, restart and
leadership return remain open.

For an ordinary new session with an initial prompt, the wrapper now opens the
named empty native chat without submitting that prompt through TUI arguments.
It preserves the initial input privately, waits for the native chat caption and
process/window visibility, checks that the thread still has no work, then submits
the input through `turn/start` without replaying a lost acknowledgement. The
ordinary-entry and quota-handoff checks passed with the synthetic provider
independently checking the first request's visible thread and controller dispatch
record. Both checks also verified visible tool results and owned process exit.
The 26 argument tests include deferred multiline text, image paths, working
directory and option separation. The ignored
`native_deferred_image_input_reaches_provider` contract check also passed on CLI
0.154.0: an owned PNG with spaces and Unicode in its filename reached the
synthetic provider as image data alongside unchanged multiline text, followed by
one successful tool effect. That check uses the parsed deferred input and native
`turn/start`; image input through the complete window launcher still needs its
own acceptance. This is initial-prompt acceptance only;
later interactive requests and resume/fork prompts remain outside this
increment. Executor/helper dispatch and closed-window recovery are covered
by the ordinary launcher checks below.

The native request-correlation check uses `client_metadata.thread_id`,
`session_id` and `turn_id`, requiring agreement with the nested
`x-codex-turn-metadata`. A fresh CLI 0.154.0 parent/child contract run confirmed
that both conversations can share `prompt_cache_key` while retaining different
thread identities. The cache key is therefore unsuitable for choosing a visible
conversation. `task_request` rejects missing or conflicting correlation; the
synthetic provider's visibility checks consume that identity. This is correlation
evidence, not dispatch authorization or a delivered transport gate. The inspected
native hook events do not establish a before-every-model-request barrier;
subsequent native requests still need authoritative visibility control.

`task_forward` provides the pending gate's one-request streaming transport using
the existing system-curl identity check and cancellable pipe/Job owners. The
[curl option contract](https://curl.se/docs/manpage.html) supports configuration
through stdin and raw streamed HTTP/1.1 output, avoiding another TLS dependency.
Owned loopback tests with system curl 8.21.0 verified unchanged request body and
test authorization, response delivery before completion, preserved HTTP 429,
HTTP 302 without redirect following, and cancellation closing the upstream and
removing the temporary request body. The streaming check also verifies unchanged
chunk framing and trailers, with delivery of the first chunk before the upstream
produces the second. Proxy CONNECT headers are suppressed using curl's dedicated
option; an actual HTTPS proxy compatibility check remains outstanding.
Credentials stay out of process arguments;
the body file lives in the private task root. The primitive accepts HTTPS or
literal loopback HTTP and caps a buffered request at 64 MiB. It does not itself
authorize dispatch and is not yet connected to the production native provider
configuration. HTTPS/native authentication compatibility and integrated
no-hidden-request acceptance remain open; chunked framing currently has owned
transport evidence, not a native provider integration receipt.

`task_admission` connects request correlation to the observed native session and
active turn, then checks the exact conversation's registered process identity and
visible window before forwarding. A parent's window cannot cover a child;
interrupted turns wait and stopped tasks reject dispatch. Focused checks verify
that missing child visibility produces no upstream connection. Only ordinary
turn requests currently have this admission path; other request kinds are
rejected pending explicit visibility support.

The release ordinary-entry and quota-handoff checks passed with CLI 0.154.0
through this gate and `task_forward` into an owned synthetic upstream. They
verified two and three admitted requests respectively, unchanged request bytes
and test authorization, native tool execution, visible final results and process
cleanup after `/quit`. Both predecessor and successor windows were inspected
simultaneously during handoff. These checks establish the fixture integration;
they do not establish production routing, live subscription compatibility or
subscribed worker/helper work. Private receipts retain the concrete runs.

The next integration adds `task_gateway`, an owned loopback listener using this
gate and transport. Native startup reads effective configuration and passes its
local route through `thread/start.config`; handoff copies that route into the
successor configuration. Focused ingress checks verify rejection without any
upstream connection and joined shutdown during an incomplete HTTP read. The
integrated ordinary release entry check passed with native tool execution,
visible final result, saved local route configuration and process cleanup.
An earlier handoff observation exposed a gap: window-presence checks reported
conversations obscured by another application as visible.
`task_view::Watch` now also checks the client rectangle against windows above it
and rejects cloaked conversations. An owned Windows-window test passed for
partial/full coverage, moving the cover away and hiding it; all windows were
destroyed by their test owner. The bounded traversal treats changing or unknown
composition state conservatively. It uses window rectangles, so transparent or
irregular overlays can also suspend admission. This follows the documented
[IsWindowVisible limitation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindowvisible)
and [window ordering contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindow).
The integrated ordinary, quota-handoff and minimize/restore release checks passed
with this change and CLI 0.154.0. The handoff windows were observed simultaneously;
all three checks showed the final result and verified process cleanup.
On CLI 0.155.1, `ordinary_launcher_opens_helper_before_its_first_request`
passed through the release entry point: lead, executor and helper panes were
established before the helper's first model request (helper title `Helper 1`,
slot >= 4), seven ingress exchanges succeeded, and the helper tool effect
occurred once. Nested helper spawn uses the native `collaboration` namespace
and a distinct helper result marker so the parent wait is not closed by the
helper's completion. `ordinary_launcher_restores_closed_executor_conversation`
also passed: closing the executor pane reported `view-restore.json`, suspended
new model requests, restored a new visible window for the same thread, and
let the in-flight tool finish without a continuation replay (four ingress
exchanges). These fixture checks still do not establish subscribed provider
work or global activation.
On CLI 0.155.1, `ordinary_launcher_opens_two_distinct_executor_conversations`
passed through the same release entry point using `collaboration.spawn_agent`
with required `task_name` values and `fork_turns=none` so Z.AI/Grok `high`
overrides apply. A catalog that omits parent `gpt-6-astra` does not register
collaboration tools. The fixture catalog therefore includes that parent slug
and both executor slugs; v2 `wait_agent` takes only `timeout_ms`.
`ordinary_launcher_hands_quota_refusal_to_visible_zai_lead` and
`ordinary_launcher_suspends_and_recovers_visible_conversation` also passed on
0.155.1 (successor present in the catalog; scoped minimize/restore). These
still do not establish subscribed work, live skill linking, or a real
consuming-project task.
An isolated core+board install from this checkout into owned temp homes linked
`team-lead`, `board-workflow` and `AGENTS.md` to the portable principles,
installed pinned `bd`, and started the ordinary linked launcher with
`--version` from a workspace outside the checkout. Live user home was not
modified. `ordinary_launcher_stops_on_explicit_user_stop` passed on CLI 0.155.1:
the emergency `request_stop` path recorded `explicit stop`, preserved the
partial file effect and issued no further model request.
The ignored `executor_succession` checks replace a managed synthetic session's
CLI process at a safe boundary through `codex exec resume <SESSION_ID>`: they
verify deferred replacement while the predecessor turn is in flight, a durable
handover record, confirmed predecessor stop, preserved partial work and
authorization, successor reload evidence from the session rollout, and the
`succession not established` report for a stale published revision or a
missing reload. They cover owned temp sessions only; interactive successor
views and production resume ingress are not part of this increment.
Set `HARNESS_CONTROL_CODEX_EXE` to the actual native CLI. Either prepare all
workspace binaries and run the test with `--release`, or set
`HARNESS_ACCEPTANCE_BUILD` to a complete immutable candidate from native `build`
and run the test driver in its ordinary profile. The latter verifies candidate
integrity and current source identity before copying its release binaries into
the owned fixture; both predecessor and succession commands use those binaries.

This implementation currently requires an explicit provider base URL and a new
session; resume/fork and implicit built-in routing return an explicit unsupported
error. It is not globally activated. Ingress accepts bounded HTTP/1.1 POST bodies
with a single Content-Length, retains up to sixteen connections, and has no
automatic request replay. Complete disconnect recovery, helper routing and
production compatibility remain required before activation.

Two additional CLI 0.154.0 route contracts identify the integration point.
`native_provider_address_override_preserves_binding` verifies that an app-server
CLI override of a custom provider's `base_url` retains its configured credential
environment key, wire API, authentication requirement and native model/provider
binding without modifying the home config. It starts no model turn.
`native_thread_route_override_reaches_only_selected_upstream` verifies the dotted
`model_providers.<id>.base_url` override in `thread/start.config`: both synthetic
Responses requests reach the selected owned upstream, the original upstream sees
none, and the native tool effect occurs once. The home config stays unchanged.
The contract now includes naming the empty thread and `thread/resume` before
dispatch; both requests still reach the selected upstream. Resuming an unnamed
empty thread failed with `no rollout found`, matching the need for the existing
startup naming step. This covers attachment to the running named thread, not
recovery after process restart or a fork.
This allows route setup after native configuration discovery without restarting
the server. Resume/fork and child inheritance still need separate checks, as do
the built-in OpenAI route and live subscription authentication. The
[configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
describes `chatgpt_base_url` as a login-flow setting; these checks do not repurpose
it as a universal model-request route.

The ingress writes private `gateway-exchange-<sequence>.json` receipts containing
completion, whether response delivery began, and an error-kind classification;
they contain no headers or body. The ordinary entry tests require one successful
receipt for each expected provider exchange. A successful fixture response alone
cannot satisfy this check. Release validation passed with two exchanges for an
ordinary task and three each for quota handoff and minimize/restore. The latter
verified no new request while minimized, retention of the unfinished tool after
restoration, continuation after that tool settled and no duplicate side effect.
These checks used synthetic Responses, not live subscription model calls.

The native two-client child/reconnect contract also passed with a per-parent
provider address override: both parent and child reached the selected owned
upstream, and neither reached the original one. The child preserved its native
identity across reconnect and performed its tool effect once.
`task_child_views` now names observed non-ephemeral children of known native
threads and queues separate windows. Out-of-order naming replies retain distinct
slots in an owned WebSocket test. While a child's first window is pending, the
inherited ingress holds its request instead of racing an immediate interruption.
The two right-side slots belong to executors; a replacement lead uses the lower
left, with the predecessor reduced to the upper left. Integrated child-window
acceptance and handoff with this layout passed on CLI 0.154.0. Slot reuse after completed
assignments is not implemented; additional children require reconciliation, so
this increment does not satisfy the full long-running orchestration requirement.
When both slots are occupied, further children now remain behind request
admission instead of terminating the observer and its existing workers. The
private `child-view-capacity.json` records waiting identities; repeated polls
preserve both assignments without sending extra native naming requests. The
owned WebSocket test covers continued control traffic, persisted waiting state
and removal of a vanished waiting identity. Automatic slot release and visible
presentation of this waiting reason remain unfinished.
`ordinary_launcher_opens_executor_before_its_first_request` passed through the
release entry point: parent and child were observed simultaneously, all four
ingress exchanges succeeded, the tool effect occurred once, and the native
child's final result and parent completion notice were visible. Closing both
owned chats also passed the controller cleanup check.
Like the native child/reconnect fixture, it pauses only the synthetic parent's
native goal scheduler; the canned responses cannot close that goal themselves.
This does not test autonomous completion of a real model-backed parent goal.
Its first integrated run found that the observer received the child's identity
in the parent's `subAgentActivity` start item without a separate child
`thread/started` notification. Discovery now follows that native item, queues
naming, and reads the child after the name is acknowledged. The provisional
record cannot authorize a request: admission waits for the native identity and
active-turn snapshot. Focused tests cover known-parent discovery and an already
running turn whose original start event preceded subscription. Named busy TUI
captions accept the observed native braille spinner so window readiness does not
wait for the held model request to finish. These corrections passed focused
checks and the integrated child-window acceptance.
The same release build passed the quota-handoff check with the stacked leader
windows: the predecessor's quota refusal and the successor's tool result were
visible together, three ingress exchanges succeeded, the effect occurred once,
and closing both chats completed cleanup. A preceding observation attempt
expired without its desktop receipt and was not counted as a pass; its owned
processes stopped before the verified rerun. These fixture checks do not cover
two concurrent executors, slot reuse, model-backed helpers or subscribed work.
The new `ordinary_launcher_opens_two_distinct_executor_conversations` fixture
selects Z.AI and Grok explicitly and requires both owned tools to observe their
peer's separate effect before completion. The release entry point passed with
all three native chats observed simultaneously, Z.AI/Grok `high` shown in their
separate views, and both tool results visible. The leader now uses native
`multi_agent_v1.wait_agent` calls with the returned child identities, receives
both completed results, and emits its own distinct final in its visible chat.
The strengthened acceptance passed with nine successful ingress exchanges,
exactly one effect per child, and owned process cleanup after closing all chats.
The selected model must also receive its matching assignment text. The parent
goal remains paused only for this synthetic fixture; native result delivery is
verified, but these passes do not establish subscribed provider work, goal
completion by an actual model or complete orchestration acceptance.
With the installed catalogue, CLI 0.154.0 selects multi-agent v1:
the [versioned native schema](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/core/src/tools/handlers/multi_agents_spec.rs)
uses `multi_agent_v1.spawn_agent`; neither the v2 `collaboration` call nor an
unnamespaced call is accepted. Correcting that fixture exposed an observer gap:
native v1 reports completed `collabAgentToolCall` spawn items, while the observer
only recognized v2 `subAgentActivity`. The observer now discovers a single v1
receiver only from a completed spawn with a matching known sender; failed,
ambiguous and unrelated items cannot authorize discovery. Native identity and
first-view admission remain separate requirements. A subsequent native run
opened both executor windows and produced both separate effects, exposing a
fixture mismatch with Code Mode's structured successful shell result; the
fixture now accepts that native result shape while still requiring exit code
zero and the exact expected output. A remaining intermittent failure is open: an earlier run
failed the parent's ingress exchange with `InvalidData` before its second
assignment. Gateway receipts now retain a bounded error detail and its stage
to distinguish request parsing, private-root opening and admission/forwarding.
The focused ingress test verifies that malformed/unowned requests preserve this
diagnostic without recording their synthetic Authorization value. The original
ingress failure did not recur in the subsequent passing runs; its cause remains
unresolved, so those passes do not prove it fixed.

The controller binds quota transfer to the exact latest failed native turn.
A newer turn invalidates a prepared transfer; a late catalog response cannot
restart it. Focused `task_handoff::tests` exercise stale history, native start
events and late responses over an owned local WebSocket, checking that ownership
stays with the previous leader and no successor request is sent.

`native_background_terminal_outlives_completed_turn` in `task_control_contract`
passed against CLI 0.154.0 with synthetic Responses: a yielded PowerShell command
remained in `thread/backgroundTerminals/list` after the native turn completed,
then left the inventory after its owned release signal. Inventory reads made no
model calls. Run this ignored check with an explicit `HARNESS_CONTROL_CODEX_EXE`;
it retains native schema, events and before/after inventory in its private root.
The handoff controller now waits for an empty, complete terminal inventory before
dispatching the successor. A focused local WebSocket test checks waiting,
unchanged ownership and dispatch after the inventory clears. The rebuilt release
also passed the ordinary-entry visible quota handoff with this gate: empty native
inventory, two simultaneous identified chats, consumed Code Mode tool result and
natural controller exit. This entry case does not yet exercise quota refusal
while a background terminal remains active. These checks do not prove an atomic
dispatch boundary against concurrent native clients or reconciliation of effects
outside the native terminal inventory.

Original failures are retained privately: daemon startup refused an elevated
terminal; the Unix listener rejected its directory's privacy; a new thread had
no resumable rollout before its first turn; a closed provider port entered native
offline recovery instead of finishing; and an early fixture inherited Windows
socket nonblocking mode. The WebSocket alternative preserves the daemon's guard.
These results do not establish subscribed model binding, global lifecycle,
leadership transfer, recovery after controller restart or measured quota benefit.

## Manager, inventory and core connection

```powershell
codex-harness deploy --source CHECKOUT [--codex-home DIRECTORY] [--user-home DIRECTORY] [--build DIRECTORY] [--state DIRECTORY] [--reset] [--all] [--preview]
cargo run --release --locked -p codex-harness -- build --source . --state "$env:LOCALAPPDATA/codex-harness-native"
codex-harness.exe check --build <directory> [--source <checkout>]
codex-harness.exe inventory --source <checkout> --codex-home <directory> --user-home <directory>
codex-harness.exe inspect-installation --codex-home <directory> --user-home <directory> [--dependency-user-home <directory>]
codex-harness.exe activate-build --state <state-directory> --build <build-directory>
codex-harness.exe recover-build --state <state-directory>
```

Check is model-free and executes no Cargo, project command or hooks. Optional
`--source` checks relocation without modifying registration. Inventory reports
core data links and names without mutation and does not yet cover the full
installer or dependency checks. Inspect-installation reads and validates legacy
metadata without changing it, distinguishing owned/adopted links.

Activate/recover manage a local candidate pointer only. They do not register
global commands or change services. Selection is serialized with builds. A
journal retains the exact previous pointer; recovery checks expected contents
and build identity. Repair that started from a damaged build finishes the
verified replacement. Unexpected edits preserve both target and journal. Old
binaries are never overwritten or removed. Ordinary consumers must reject an
unfinished selection journal or stale build. An integrity-verified manager may
select a repaired build even if the old binary was altered; recovery refuses to
restore an altered previous build as usable.

Core `install|update` deliver the freshest integrity-verified published build
of the running manager's owned state when `--build` is omitted; an explicit
`--build` overrides that choice, and a manager outside an owned state keeps
requiring it. Delivery only re-points the `harness/bin` links, so a running
manager file is never replaced: already started sessions keep their build and
the next session resolves the delivered one. Code-tools registrations name that
stable manager link instead of a frozen build path.

The shared Serena broker keeps one private location per delivered
build generation, so a new session serves from its own manager while sessions
of the previous build keep their broker until they finish. A generation with no
consumers retires on its idle timeout; explicit retirement remains a separate
maintenance command.

Every compilation uses a fresh owned temporary target with a short path for
MSVC; unchanged candidates reuse verified immutable binaries. Explicit release
compilation uses the [shared heavy-command budget](#heavy-command-budget).
Abandoned management scratch (the `hcb-`/`hcc-`/`hca-` temp prefixes) is
reclaimed at the next explicit build once older than 48 hours; only ordinary
prefixed directories are removed and reparse points are skipped. Retained
verification targets from soak or long checks are removed after their
conclusions are recorded (for example `cargo clean`), so verification state
does not accumulate on the system drive.
Reusing Cargo's
mtime cache across changed source hashes is unsafe even if the manifest uses
content hashes. Changing native source/lock inputs makes Check stale while
launches keep following the integrity-verified delivered build; an explicit
build/update (deploy) switches new processes to the newer source.
Documentation-only changes do not require compilation. Missing or altered
manager binaries require explicit Cargo bootstrap and still refuse runtime.
Executable text resources
belong under crate `src`; source-owned skill/configuration data remains live
filesystem input and must not be embedded in a deployed binary. Source
subdirectories named `tests` or `examples` are included. Root documentation
and noncompiled Markdown outside `src` do not require recompilation.

The source fingerprint includes non-Markdown files throughout `crates/`,
including integration tests and their support modules. Freeze those inputs as
well as runtime code during an explicit native build. A successful compilation
is rejected if that fingerprint changed before finalization; finish source/test
edits and their focused checks before starting another immutable candidate.

Native core `install` / `update` / `check` / `recover` / `disconnect --core-only`
exist. A model-free Process PATH cycle connects, repeats, checks and disconnects
while preserving unrelated token-workflow and subscription records. Default PATH scope is User; optional `--path-scope User|Process` selects
a new connection's scope. An existing installation keeps its recorded scope; an
explicit differing scope requires disconnection first. Process PATH belongs to
the current manager command and descendants and cannot change the calling
PowerShell environment. Owner identity is PID plus creation time. Mutation uses
the Process PATH module mutex. Journal schema 11 remains read-compatible with
schemas 8/9/10. Current PATH receipts retire atomically with their terminating
recovery record. Historical files without a live intent/commit are preserved.
`recover --core-only --preview` reports rollback, finish or no pending operation
without writing. Native `connect` retargets recorded source links after a moved
checkout without opening the old source. Relocated old-layout preview and mutation
upgrade owned script links, preserve an adopted profile, and refuse a foreign
hook replacement. Interrupted relocated old-layout connect leaves a native
journal; Recover restores prior owned script links and the adopted profile
without opening the old source. Combined component recovery, real-CLI
relocation, final private-state cleanup and ordinary global installation remain
unfinished.
Registration currently requires local NTFS with TxF.

Mutually exclusive `--core-only`, `--code-tools-only`, `--subscriptions-only`,
`--token-workflow-only` and `--board-only` are accepted. Combined activation is
refused.
`--code-tools-only` Check/preview/Install reuse adopted packages.
Update rewrites owned MCP registrations from
adopted inventory without acquiring packages or stopping shared OpenCode
consumers. Subscription and token-workflow Check/preview inspect owned records
only and do not query or stop the live routing proxy. Code-tools Recover/Disconnect
reuse the native MCP
journal. Token-workflow Recover/Disconnect restore or remove only recorded
`harness/bin/rtk.exe` and `harness/bin/harness-rtk.exe` links, preserve foreign
files, and do not follow link targets. When a recorded original CLI is present,
they also edit an ordinary `config.toml` as described below. Subscription
Recover/Disconnect restore or remove owned routing files, source links and an idle Task Scheduler
definition without querying or stopping the live proxy. Subscription
Install/Update write those owned files and source links
and may register an idle Task Scheduler definition without starting the live proxy;
Recover can restore an interrupted restart-policy journal in place without
stopping the live proxy. `configure-restart --subscriptions-only` updates the
owned Task Scheduler restart count/interval in place without starting or
stopping the live proxy.
The scheduled action is `codex-harness subscription-service --state FILE`.
Isolated fixtures cover readiness, 2048 MiB job assignment, shutdown cleanup
and runtime retry. `codex-harness subscription-login xai|zai` is the
native login entry: browser-only xAI OAuth and a host-private Z.AI key store
with an owner-only ACL. The subscription lifecycle owns the native xAI
profile and catalog (schema-version 2); legacy OpenCodex records are retired
behind a recovery journal. Disconnect removes the native wiring. Isolated
fixtures never target the live account.
Token-workflow Install reuses or acquires the pinned RTK archive into
`harness/rtk/packages/<version>/rtk.exe` and a bounded adapter build under
`harness/rtk/build/<identity>/harness-rtk.exe`, then links only those two
`harness/bin` destinations. Preview creates no links. When
`harness/installation.json` records an original native CLI, first-connect
Install enables `code_mode` then `hooks` on an ordinary `config.toml`,
Disconnect disables `hooks` and restores recorded `previousCodeMode`, and
Recover of a not-enabled previous state disables `hooks`. Feature edits are
skipped when that executable is absent and refuse a reparse `config.toml`
without following it.
`cargo test --locked -p harness-core --test serena --jobs 1 -- --test-threads=1`
rejects missing registry, missing Python and incompatible version/status before a
child starts. Adopted-package probes need `HARNESS_CODE_TOOLS_REGISTRY` and
`--ignored`. The helper sets an owned `SERENA_HOME` under the launch home and
does not replace the live Python seam. Post-delivery MCP availability is also
checked through the real entrypoints with a location record grown past the
historical fixed bound: the unignored broker-state and Serena-broker
regressions run in the standard workspace gate, and the adopted-package check
`cargo test --locked -p codex-harness --test serena_stdio grown_location_record_still_completes_handshake_after_deliveries -- --ignored --exact --test-threads=1`
(explicit `HARNESS_CODE_TOOLS_REGISTRY`) must complete MCP initialize and
tools/list before a release is called delivered.

## Diagnostics and dependencies

`dependencies discover` and `dependencies plan` remain read-only; discovery
executes no package code, and planning reads official release metadata through
system curl. `dependencies stage`, `validate`, `select`, `selected`,
`recover-selection` and `rollback-selection` connect explicit candidates to
bounded runtime validation and journaled local selection. BasedPyright
requires an explicit Node path and digest. `dependencies apply|update`
dispatches explicit plan actions: Check and preview stay read-only; Apply
stages BasedPyright with an explicit Node path and digest, and installs
missing nuphus packages into the shared user npm tree through a native
journaled create-directory transaction with audited official archives,
probed executables, recorded provenance and rollback to prior absence on
failure. Existing installations, a shared npm lockfile and foreign markers
are preserved without mutation. `dependencies probe --executable FILE --kind
nuphus --sha256 DIGEST` checks a digest-pinned native MCP: Nuphus lists its
protocol/tool contract without desktop or browser actions. The caller must
establish executable and companion DLL provenance first.
`dependencies recover-npm --state DIRECTORY [--rollback-committed]` finishes
or rolls back interrupted shared-npm activation journals.

`mcp serena` and `mcp nuphus` serve the retained integrations through
first-party stdio adapters. `mcp prepare-mcp` plans native projections for
the installer, `mcp apply-registration` journals the owned MCP registration
block (including removal of retired owned registrations), and
`mcp broker-prepare` / `mcp broker-retire` manage explicit broker roots.
Retired CodeGraph, Codebase Memory and Graphify commands no longer exist;
host residue is inert.

## Outcome, usage and helpers

Installed tool-workflow qualification for the retired graph tools was removed
with their code. Serena and Nuphus operation coverage lives in the
`serena_stdio` adopted-package checks and the Nuphus probe contract above;
new installed-consumer checks belong to the change that needs them.

`codex-harness.exe outcome-report --input <private-json> [--markdown]` formats
local attempt accounting without executing anything.
`delegation-usage` reads explicit local telemetry without model calls.
`token-audit` scans rollout sessions through the shared reader with
`cargo test --locked -p token-audit --jobs 1 -- --test-threads=1`; its
reader changes also rerun
`cargo test --locked -p codex-harness --test delegation_usage --jobs 1 -- --test-threads=1`
because both consumers share `harness-core::rollout_reader`.
`outcome-run --request PATH --run-model-probes` executes one explicit native
launcher in an isolated temporary case/home and defaults to a model-free skip.
`outcome-prepare`, `outcome-oracle`, `outcome-discover` and `outcome-arm`
cover the five local controlled cases, independent oracles, model-free
discovery and both skill comparison modes. `tools/outcome_native_pairs.py` is
the explicit opt-in driver for one native baseline/candidate pair per local
case; it spends ChatGPT quota only with `--run-model-probes` and keeps
evidence in a private temporary root. One such pair later ran for each local
case on hooks-off Astra/xhigh. Benefit remains unproven. Two-consumer
quantitative comparison is out of this change's acceptance (2026-09-18).
Verification-skill global lifecycle is delivered. Build the
workspace binaries first (`cargo build -p codex-harness --bins`): the legacy
pair driver's independent process oracle now runs commands through native
`harness-observe.exe`.
`tools/outcome_cases.py --prepare --primary PATH --secondary PATH` copies two
explicit local checkouts into an owned inputs root as generic `primary` and
`secondary` snapshots. It does not discover neighboring repositories by
directory name. Do not pass this checkout as a consumer. Model-backed
two-consumer benefit pairs are not authorized by `accelerate-verified-delivery`.

The native launcher and argument policy preserve upstream-managed background
process lifetime, package-manager metadata, argv, Unicode, streams, exit codes,
task-effort options and Ctrl+C. Isolated tests refuse self-recursion; a changed
registered upstream still launches without a compatibility warning. The actual
global launcher still uses its existing script. Native profile/config
precedence follows the
[official configuration contract](https://learn.chatgpt.com/docs/config-file/config-basic#configuration-precedence).
The launcher also keeps executor sessions single-agent: while the harness
executor marker is present in the environment, it adds
`-c agents.enabled=false` before forwarding, so every Codex process started
from an executor - including a raw nested `codex` invocation - runs without
the agent tool set.

## Compatibility during concurrent changes

Preserve all `linked-global-kit` source-update scenarios and both pending
deltas at each sync. Executable helpers follow build integrity; source-owned
skill descriptors remain live; accepted skill revisions must be delivered in an
existing session without reloading the entire initial prompt. Ordinary hooks
remain off. The accepted RTK exception and explicit Serena Python/Rust
selection are preserved. The current resource selection must survive every
native port: shared project-isolated Serena workers with their Job and inner
CPU caps under the [shared account allowance](#shared-agent-cpu-allowance),
and ownership-aware cleanup. Retired graph tools impose no runtime
resource selection anymore.

The original RTK script lifecycle now reads the root workspace manifest/lock
and builds only `harness-rtk`. Native RTK lifecycle and Rust acceptance-helper
exist. `cargo test --locked -p codex-harness --test rtk_adapter --jobs 1 -- --test-threads=1`
covers exec-once, hook rewrite, malformed/Stop silence, disable/missing
bypass and oversized raw passthrough after `cargo build --locked -p harness-rtk --jobs 1`.
