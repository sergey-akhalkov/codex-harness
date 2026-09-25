## 1. Content sourcing without command-line limits

- [x] 1.1 Add one bounded UTF-8 stdin reader (pipe/terminal discrimination, BOM handling, 8 MiB ceiling) shared by both message commands, and verify with native tests that piped content is read literally, an empty stream is refused as no content, a terminal stdin never blocks, invalid UTF-8 is refused, and the ceiling error names the actual limit.
- [x] 1.2 Wire the reader into `executor message` and `lead message` as the source when no content flag is given, plus explicit `--text -`, keeping `--text TEXT` / `--file FILE` semantics unchanged; verify flag precedence, the not-both refusal and existing argument tests still pass.
- [x] 1.3 Accept `--exec -` in `executor spawn` through the same reader and verify a streamed assignment renders the standard brief, allocates the slot only after a nonempty stream, and terminal/empty stdin refuses without allocation.

## 2. Automatic oversized-message delivery

- [x] 2.1 Implement the spill owner: persist the complete payload to `CODEX_HOME/harness/messages/<message-id>.txt` above the unchanged 256 KiB inline bound, compose the compact pointer envelope (size, absolute path, read instruction), and verify byte-exact persistence without shell evaluation for both directions and the `--reply-to` form.
- [x] 2.2 Extend message receipts and watch output with the spilled result class (`delivery: "spill"`, `payloadPath`, `payloadBytes`) and verify a spilled send is never reported as inline delivery, appears in the existing watch surfaces, and an inline-sized message produces byte-identical delivery evidence to today.
- [x] 2.3 Integrate spilled envelopes with the existing content-identity and duplicate-suppression logic and verify a retry after an indeterminate spilled result cannot deliver twice, a definite refusal leaves no phantom record or reply hold, and a repeated identical payload after a resolved exchange starts a new identity.
- [x] 2.4 Tie spill-file cleanup to run-record cleanup with a bounded messages directory and verify files disappear with their owning run records, unrelated messages survive, and directory-bound degradation is an honest error rather than silent eviction.

## 3. Recorded-identity address defaults

- [x] 3.1 Implement the default resolver (flag > `CODEX_HOME` > installation default; `--source` from the installation record; run identity from live leases/receipts) and verify explicit flags always win, resolution reads only harness records, and no cwd/process/window/most-recent-session inference exists in the path.
- [x] 3.2 Apply the resolver to `executor message`, `executor stop` and `executor watch` and verify the unique-live-run case resolves and passes identical verification, several live runs produce a bounded listing naming `--slot`, and zero/stale runs produce the existing honest state errors with no send or stop.
- [x] 3.3 Verify the defaulted forms reject cross-checkout and cross-home confusion: two sources sharing a home, two homes sharing a terminal, and a resolved slot later reused by another run all refuse with the observed mismatch.

## 4. Instructions, help and compatibility

- [x] 4.1 Update CLI usage/help and the generated executor brief to teach piped content, automatic spill and address-free steering as the first-choice forms, and verify source and installed help agree, explicit forms remain documented as fallbacks, and no surface instructs creating a temporary file for a long message.
- [x] 4.2 Update `.agents/skills/team-lead/SKILL.md`, `docs/agent-delegation.md` and `docs/rust-native.md` in their existing homes, and verify local links, watch/receipt field names and the message-versus-stop selection rules stay intact.
- [x] 4.3 Measure the combined generated-brief/skill/reference/help load for the same assignment inputs and verify it does not grow relative to the pre-change baseline, keeping the accounting alongside the `add-executor-lead-messaging` instruction-load task.

## 5. Checks and acceptance

- [x] 5.1 Extend the native delegation suites with model-free coverage for stdin sourcing, threshold spill, ceiling refusal, pointer-envelope delivery evidence, defaults/ambiguity/staleness and `--exec -`; run the affected executor spawn/message/stop/watch/assignment suites plus formatting, source diagnostics and executable ownership checks, and report unavailable or stale diagnostics separately from executed tests.
- [x] 5.2 Compose with `add-executor-lead-messaging`: after its archive, reconcile this change's modified requirements against the then-current main specs and run strict OpenSpec validation for proposal, specs, design and tasks.
- [ ] 5.3 Deploy through the existing harness installation lifecycle and verify a fresh consumer outside this checkout completes a piped oversized steering round trip and an address-free reply through the installed launcher, with the recipient reading the spill file named in its conversation; preserve prior-build recovery.
