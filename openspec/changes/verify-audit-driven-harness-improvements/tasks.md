All checkboxes below describe future implementation or investigation work. Planning validation does not complete them. Finding IDs and requirement aliases resolve in [design.md](design.md#audit-coverage-matrix). Use the existing native test/evidence owners; preserve private inputs outside tracked source.

Dependencies: group 0 establishes current inputs. Groups 1, 2, 3, 6, 7 and 11 can progress independently where runtime ownership permits. Group 4 depends on baseline accounting decisions in group 3; group 8 builds on group 5 and preserves group 6 ownership. Group 9 establishes the evidence contract before group 10 comparisons. Group 12 can wire deterministic coverage as each slice lands; its installed integration and group 13 consume completed slices. Group numbering is an ownership map, not a requirement to postpone a serious endpoint or deletion defect behind an optimization.

## 0. Freeze relevant inputs and exercise the supported environment

- [x] 0.1 Record the implementation base revision and affected dirty inputs, map the current native entry points/tests to this change, and verify the audit's source predicates still hold; preserve any already-fixed item with its actual regression evidence instead of reimplementing it.
- [x] 0.2 Establish applicable Rust navigation, source coverage, toolchain and native check commands through the installed capabilities; verify one representative retained navigation operation and one small applicable native check, or retain a cause-bearing unavailable state linked to group 11.
- [x] 0.3 Freeze synthetic input/oracle ownership and local evidence locations for concurrent storage, transport and usage cases; verify fixtures cannot affect unrelated user state and candidate code cannot rewrite authoritative expected outcomes.

## 1. Correct RTK publication and complete presentation

- [ ] 1.1 Add controlled two-process and interruption reproductions in the existing RTK integration owner for `crates/harness-rtk/src/main.rs`; verify the original publication/retention interleaving fails the declared handle invariant and preserve deterministic barrier inputs. Covers A01.
- [ ] 1.2 Implement short coherent publication/retention and consistent recall with recoverable staging and bounded process-safe ownership; verify both successful handles below retention limits, crash-before/after-commit, reader/eviction overlap, lock/write failure, digest correctness and exactly one child execution. Covers A01.
- [ ] 1.3 Validate index schema, handle/path grammar, digest/size metadata and accounting before destructive retention; verify malformed/unknown metadata preserves committed evidence and parent-relative/absolute/reparse sentinels outside the owner survive. Covers N01.
- [ ] 1.4 Prepare and account for full presentations across `filter`, `present_stream` and `run_cargo`, including both streams, locators, handles, exit status and emitted adapter overhead; verify actual applied bytes shrink on the same basis diagnostics report, raw fallbacks preserve necessary faults/progress, and long-path/almost-incompressible/non-Cargo cases pass. Covers A02, N07.
- [ ] 1.5 Correct Cargo status-layout recognition without changing raw recovery or supported command behavior; verify wrong-indentation/status-like user output, unknown diagnostic blocks, stderr failures, binary/oversize/machine-output bypass and exactly-once execution. Covers A03.
- [ ] 1.6 Run the relevant existing RTK adapter suite through the documented heavy-command/native route and exercise installed raw/recall behavior outside the checkout; verify stream identity, original bytes, child status and retention limits remain compatible after the integrated RTK changes.

## 2. Preserve effective native model and effort

- [x] 2.1 Qualify the installed native configuration-resolution and provenance contract against `crates/harness-core/src/portable_config.rs`, `launcher.rs` and `native_launcher.rs`; verify actual user/project/profile/CLI precedence and trust behavior with owned synthetic configuration before selecting the smallest supported integration. Covers A04, N04.
- [x] 2.2 Change default injection so an applicable native model/effort choice survives and the existing per-model mapping applies only to absent effort; verify the saved model/low-effort counterexample through final launch arguments and actual native effective configuration. Covers A04.
- [x] 2.3 Exercise TUI and exec, nested trusted/untrusted project configuration, CLI overrides, profiles, remote/compatibility selectors, missing effort, unknown models and malformed optional defaults; verify source-of-value diagnostics, native fallback and one payload launch without exposing private values. Covers A04, N04.
- [x] 2.4 Reconcile the existing operational guidance for saved preferences and fallback mapping in its owning docs/skill only where behavior changed; verify examples against the installed path and retain global live-data and provider-selection constraints.

## 3. Repair baseline persistence and comparison

- [ ] 3.1 Fix returned-name resolution and native Codex-home fallback in `crates/token-audit/src/baseline.rs`, using its existing CLI tests; verify `save -> diff` with returned filename/basename, unset CODEX_HOME, valid latest and rejected escaping names/pointers. Covers A05, A08.
- [ ] 3.2 Publish uniquely identified immutable snapshots and a coherent latest pointer under concurrent and interrupted writers; verify distinct same-second saves survive and latest never refers to a partial file or overwrites another successful snapshot. Covers A09, N02.
- [ ] 3.3 Validate the complete snapshot before compatible status, separate format validity from population comparability and version changed semantics; verify expected-schema/missing-fields, corrupt JSON, foreign schema, root/window/mode mismatch and inspectable legacy snapshots without fabricated baseline zeros. Covers A06, A07.
- [ ] 3.4 Carry usage basis, partial-subtotal status, warnings and relevant coverage through snapshot and diff projections; verify lost usage cannot masquerade as savings and mixed/unknown counters stay labeled in text and machine output. Covers A31, N03.
- [ ] 3.5 Add documented bounded text diff and stable detail/paging through the existing report/detail owner; verify several thousand session/group movements, byte/row limits, deterministic selection, hidden counts, complete detail and totals equal to exhaustive JSON. Covers A10.
- [ ] 3.6 Run the baseline/report/CLI compatibility targets and a local installed save/diff/detail path with owned synthetic sessions; verify upgrade and recovery preserve original snapshots and unsupported comparison metadata stays explicit.

## 4. Add honest interval usage and safe incremental analysis

- [ ] 4.1 Define versioned explicit activity-window and interval-usage output, UTC boundaries, event timestamp basis, cumulative reset/gap policy and coverage in the analyzer's existing model/CLI owner; verify the design with synthetic old-plus-new, boundary-straddling and absent-timestamp records before dependent parsing work. Covers A11, A31.
- [ ] 4.2 Extend `crates/harness-core/src/rollout_reader.rs` and `crates/token-audit/src/report.rs` for identifiable interval usage, stable-response deduplication, retries and supported model/effort attribution; verify the shared delegation-usage acceptance remains unchanged. Covers A11.
- [ ] 4.3 Add native CLI cases for activity lifetime totals versus in-window usage, duplicates/conflicts, cumulative resets, missing data, model switches and day boundaries; verify unknown allocation remains unknown and no session-start bucket is mislabeled spending. Covers A11.
- [ ] 4.4 Qualify a reliable unchanged-prefix/snapshot identity mechanism for incremental reuse using existing filesystem/lifecycle capabilities; verify append, same-size preserved-time mutation, replacement and unavailable change evidence, and record full-parse fallback wherever correctness cannot be established. Covers A12, N09.
- [ ] 4.5 Implement versioned disposable local parser checkpoints and incremental reuse only for proven eligible inputs; verify equivalence with full scans on append, partial tail, truncate, replace, earlier-prefix mutation, corrupt cache, parser change and concurrent input boundaries. Covers A12, N09.
- [ ] 4.6 Measure bytes read, events parsed, invalidation reasons and elapsed time on unchanged eligible history and active tails; verify reduced reads with equal reports/findings/coverage, disclose cache preparation and recovery costs and retain scoped claims if wall-time benefit is unresolved. Covers A12.

## 5. Shorten installed launch without weakening integrity

- [ ] 5.1 Separate runtime integrity/compatibility from explicit source freshness in `crates/harness-core/src/build_identity.rs` and its `build_selection.rs`/`native_launcher.rs` callers; verify ordinary launch avoids compiled-source freshness traversal while required live shared data still applies. Covers A13, N08.
- [ ] 5.2 Reuse coherent executable verification within one decision and remove duplicate manager hashing without a persistent mtime trust cache; verify a changed/ambiguous executable invalidates the reused result and all required binaries remain checked. Covers A14.
- [ ] 5.3 Reject source/toolchain/target mismatches in `crates/harness-core/src/native_build.rs` before expensive consumer validation; verify nonmatching candidates are skipped and a selected matching candidate still passes full required validation. Covers A15.
- [ ] 5.4 Exercise actual launch/check/diagnose paths with healthy, stale, unavailable and altered inputs; verify accurate explicit freshness, measured launch reads, preserved live defaults and exactly one valid upstream fallback rather than a total Codex outage. Covers A13, N08.

## 6. Confine scratch reclamation to abandoned owned work

- [ ] 6.1 Add native ownership counterexamples for the existing scratch sweep using old foreign prefixed directories, an old active operation and outside reparse sentinels; verify expected preservation is independent of matching name/age. Covers A19.
- [ ] 6.2 Introduce the dedicated short owned scratch root and validated live lease in the existing native-build lifecycle; verify abandoned owned data is reclaimed, active/foreign/reparse data survives, PID reuse is not ownership and cleanup failure does not fail a valid build. Covers A19.
- [ ] 6.3 Preserve legacy unproven scratch and document bounded recovery in its existing owner; verify repeat build/update and interruption recovery retain short Windows paths and do not sweep neighboring TEMP namespaces. Covers A19.

## 7. Protect xAI transport ownership and generation continuity

- [ ] 7.1 Define and implement owned endpoint/control identity using existing process/build/generation owners in `crates/harness-core/src/native_launcher.rs` and `xai_responses_shim.rs`; verify a self-reported valid-looking executable path alone does not authenticate a listener and stale control authority cannot retire another generation. Covers A20, N05.
- [ ] 7.2 Reject unknown/spoofed listeners and distinguish free, occupied and probe-unavailable states; verify synthetic secret-bearing requests never reach an unverified endpoint, foreign processes survive and failure preserves the requested provider/billing route. Covers A20, N05.
- [ ] 7.3 Bind new and existing sessions to verified compatible generations and retire by live ownership/work release; verify an ordinary update selects a new endpoint without invalidating an existing session's next request. Covers A21.
- [ ] 7.4 Exercise real owned loopback processes with a stream continuing beyond the former grace, a concurrent new session and a later request from the old session; verify original request deadlines, correct generation selection and no production model call. Covers A21.
- [ ] 7.5 Exercise concurrent launch/update, unclean owner exit, explicit forced recovery and rollback; verify only abandoned owned generations are reclaimed and unrelated live requests/processes remain intact. Covers A21.

## 8. Improve development feedback and evaluate publication candidates

- [ ] 8.1 Exercise and document the smallest sufficient existing dev/package/integration route for a representative small edit, retaining required broader checks; verify a useful check through the real affected entry point without first publishing a cold release. Covers A16.
- [ ] 8.2 Compare source-identity inputs with actual compiler dep-info/build/configuration inputs; verify compiled includes beneath fixture/test/documentation paths remain tracked and record whether a narrower safe publication key is justified before changing it. Covers A17.
- [ ] 8.3 Declare and run bounded native comparisons for justified build-key, reusable-target or compiler-profile candidates using the same source/toolchain/resource conditions; verify unchanged publication integrity and retain the current route when a candidate has no established safe benefit. Covers A18.
- [ ] 8.4 Record separate cold/warm feedback, publication, startup, runtime and artifact measurements plus preparation/recovery cost in the existing owner; verify each optional candidate has an evidenced adopt/reject/inconclusive decision and only accepted candidates reach the installed path. Covers A18.

## 9. Connect evidence to a defensible benefit decision

- [ ] 9.1 Add counterexamples in the existing `benefit_gate.rs`/`outcome_report.rs` test owners for consistent-without-benefit, tolerated regression, unverifiable evidence and stale input identities; verify none can be labeled demonstrated improvement merely by arithmetic or an accounting string. Covers A23, A24.
- [ ] 9.2 Extend existing attempt/comparison accounting with explicit independent pair/block and retry identities, complete attributable leader/worker/rework costs and coverage; verify Cartesian comparisons are not independent samples, overlapping time and inherited summaries are not double counted, unknown usage stays unknown and zero accepted tasks produces an undefined ratio. Covers A25, A26, A31, A39.
- [ ] 9.3 Link actual execution/consumption identity, protected independent oracle, comparability and computed effect through the existing evidence owners; verify changed oracle/input/skill/model/effort and absent model-visible evidence cannot silently authorize adoption. Covers A23, A30.
- [ ] 9.4 Add predeclared task mix/effect/nuisance/stopping/uncertainty and net-horizon semantics to the existing comparison route; verify deterministic quality improvements, stochastic savings and unmeasured subscription claims are classified separately without retroactively changing historical thresholds. Covers A24, A39.
- [ ] 9.5 Record the reviewed no-migration and no-blind-pruning selections with primary evidence and a middle-diagnostic counterexample; verify no optional runtime/plugin/default is installed under an unproved benefit assertion. Covers A35, A36.
- [ ] 9.6 Review the corrected owning boundaries for validated snapshots, prepared presentation, committed observations and verified runtime; verify each specific invalid transition is covered by its native regression without introducing an unused generic framework. Covers A37.

## 10. Verify actual instruction consumption and evaluate context candidates

- [ ] 10.1 Strengthen the existing skill-isolation/evaluation owner to identify catalogue, body, references and helpers actually consumed and deny or detect live-library fallback; verify intended-case contamination and missing treatment are explicit while a valid negative case can avoid activation. Covers A27.
- [ ] 10.2 Exercise intended, similar-unsuitable, boundary/failure, independent held-out and protected overlapping workflows with frozen acceptance; verify contamination, reference drift and a skipped mandatory check cannot authorize the candidate. Covers A27, A28.
- [ ] 10.3 Select measured recurring candidates for instruction mechanics/layout, stable-prefix rendering, native deferred discovery and deterministic Code Mode/native aggregation; freeze each treatment and acceptance plan, verify installed support and actual presentation observability, and record unsupported candidates without introducing a replacement runtime. Covers A29, A30, A32, A33, A34.
- [ ] 10.4 Execute only the explicitly selected relevant comparisons after group 9 and consumption isolation pass; verify complete accepted-task cost, required discovery, errors/detail recovery, cache basis, intended/negative/held-out behavior and uncertainty. A model-backed case not yet authorized or run stays pending, not passed. Covers A28, A29, A32, A33, A34.
- [ ] 10.5 Integrate only evidenced accepted owned candidates and record rejection/inconclusive decisions with conditions for reconsideration; verify the prior defaults remain for unproven candidates, authority/recovery/completion rules survive and external OpenSpec instructions are unchanged. Covers A29, A32, A33, A34.

## 11. Restore and truthfully report semantic readiness

- [ ] 11.1 Preserve local bounded failure evidence and reproduce the affected managed operation for `crates/harness-core/src/serena.rs`, `serena_shared.rs` and configuration ownership; verify project coverage, relevant backend inputs and actual resource evidence distinguish the observed OOM from its still-unproven cause. Covers N06.
- [ ] 11.2 Apply the smallest cause-supported correction in its existing owner and make readiness/failure states useful; verify the existing bounded retry is not duplicated, unresolved initialization cannot be reported as working semantics and required languages/resource policies are not silently weakened. Covers N06.
- [ ] 11.3 Exercise representative retained Rust and affected multi-language navigation through the actual managed installed consumer outside the checkout; verify current coverage, useful failures, unaffected operations and separately qualified diagnostic freshness. Covers N06.

## 12. Deliver reproducible Windows verification

- [ ] 12.1 Add the public Windows/MSVC workflow using the existing native check commands, identified toolchain, locked inputs, immutable action revisions and minimal permissions; verify local workflow/configuration validity and that untrusted PR execution cannot use production credentials or mutate the owner's installation. Covers A22.
- [ ] 12.2 Connect deterministic audit regressions and applicable formatting, lint, test, ownership and source/link checks; verify intentional failures propagate nonzero status and omitted/unavailable checks cannot appear green. Covers A22.
- [ ] 12.3 Define and exercise the separate real launcher/install/update/recover/transport/MCP integration route, including an outside-checkout consumer; verify synthetic default CI is not reported as equivalent to unexecuted installed acceptance and model-backed work remains explicitly selected. Covers A22.
- [ ] 12.4 Obtain an authorized actual CI run tied to the candidate revision and preserve bounded evidence; verify executed scope and private-data sentinel handling in public artifacts. If remote publication/settings authority or runner capability is unavailable, leave this execution task open with the exact blocker. Covers A22.

## 13. Integrate, deliver globally and close on actual evidence

- [ ] 13.1 Exercise a small real installed user path through repaired RTK recall, native launch and local analyzer report/baseline/detail before expanding the integrated acceptance; verify actual consumption from outside the checkout and preserve all remaining agreed tasks. Covers A38.
- [ ] 13.2 Deliver through the existing global lifecycle and verify update/check/recovery/rollback, active session continuity, versioned local evidence and unrelated state preservation; verify the delivered manager and live shared data belong to the accepted source/build without repository-only completion claims.
- [ ] 13.3 Reconcile every A01-A39 and N01-N10 matrix row with current requirement/task/evidence status, correct obsolete owning documentation and validate the OpenSpec change and local links; verify no mandatory row closes on static inspection, no optional verdict is mislabeled an improvement, original citation placeholders are gone and no universal benefit/coverage claim exceeds the inspected scope. Covers A38, A39, N10.
- [ ] 13.4 Run the applicable complete native checks from `docs/rust-native.md` on the integrated candidate, including executable ownership and source hygiene; verify exact identities, required failed-path regressions and affected installed checks, reusing unchanged valid evidence without unrelated repeated gates.
- [ ] 13.5 Review rollback and the complete decision vector before archiving: required corrections and global delivery pass, optional investigations have honest decisions, outstanding authorization/execution remains open, and measured bytes/I/O/time/tokens never become an invented quota percentage; verify all completed checkboxes are backed by their stated acceptance.
