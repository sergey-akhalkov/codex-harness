# Bounded code retrieval

Choose the smallest operation that answers the question. Check the active root and relevant language/index coverage once, then reuse it while unchanged. CodeGraph below is conditional on actual selection; a proposal is not an installed MCP.

| Need | Start with | Initial scope |
| --- | --- | --- |
| Exact symbol in a known file | Serena `find_symbol` | `relative_path`, exact name, `depth: 0`, `max_matches: 1`, `include_body: false`, `max_answer_chars: 4000` |
| Read one function | Serena `find_symbol` | Same scope, `include_body: true`; expand the answer limit only if that body is needed |
| File structure | Serena `get_symbols_overview` | One file, `depth: 0`, `max_answer_chars: 4000` |
| Exact references / edit safety | Serena references and applicable source checks | One symbol/file, `max_answer_chars: 4000`; a too-long answer requires narrowing or deliberate bounded expansion |
| Unknown location / direct graph relationships | CodeGraph search or callers/callees | Exact term, five matches, file disambiguation where supported; no automatic body reads |
| Impact candidate set | CodeGraph impact | One unambiguous symbol, `depth: 1`; confirm ambiguous or consequential edges |
| A specific unresolved flow spanning files | CodeGraph explore | Explicit names, `maxFiles: 1` or `2`, retained result and selected evidence before model delivery |
| Literal text, config, documents, unsupported source | Scoped `rg` / native read | File or directory and bounded matches/lines |

For diff impact, establish the exact base and whether the input includes staged,
unstaged or untracked work. Map changed code to unambiguous symbols before impact
queries; a graph does not define the Git diff. Review changed configuration and
dynamic consumers with current source. Select a caller/entry-point check that
would fail if the changed contract broke. A list of graph callers alone does not
validate the change.

For architecture, start with a named subsystem/file and one-hop relationships.
Use explore only for a specific remaining cross-file question, with explicit
`maxFiles`. For value tracing, inspect the relevant assignments/arguments/returns
in current source: call edges show possible relationships, not value propagation.
Confirm whether the selected server actually offers meaning-based search. Name
matching is not semantic search; when semantic discovery is unavailable, derive
candidate names from the required behavior, use scoped structural/literal search
and check candidate bodies. Report that coverage limit instead of a negative
claim about the implementation.

Serena's `initial_instructions` can advise trusting a refactor without checks.
Keep the consuming project's applicable behavioral checks: tool success proves
the operation's reported execution, not correct caller behavior. Empty diagnostics
remain unverified unless their current complete analysis is independently known.
Likewise, a mode called `planning` is not an enforced read-only boundary. Qualify
the effective catalogue and rejected owned writes of the chosen MCP restriction;
native tools still retain the user's separately authorized capabilities. Existing
clients may keep an older catalogue after configuration changes; distinguish a
fresh consumer's discovery from an in-turn change.

In the qualified Serena 1.7.0 installed route, a project configured with
`read_only: true` and a fixed set of read tools rejected both symbol replacement
and memory writing. The shared proxy still advertised mutators before dispatch
to that project's worker. Qualify the rejected operations and unchanged bytes;
do not describe the proxy catalogue itself as a filtered read-only interface.
This is a selected MCP configuration, not an instruction to reconfigure tools
for every read-only task or an operating-system sandbox.

For a small question, start with 4 KiB per delivered answer and 8 KiB cumulative retrieval. Explain the missing fact before increasing breadth; deliberate responses remain at most 16 KiB. These are output budgets, not tokenizer or subscription measurements. Until a managed adapter enforces them, use Code Mode to retain the raw result and return a bounded projection. Check all settled outcomes, protocol errors and `isError`; preserve warnings, project/generation, omitted counts when known and a detail locator. Do not present a cut source body or list as complete. An expired locator is an explicit limitation, not permission to rerun silently.

CodeGraph 1.6.0 traps verified during replacement planning:

- `maxFiles=1` still returned 15,679 bytes; `maxFiles=2` reached 24,983 bytes. File count is not an answer budget.
- A common-name caller query returned 14,984 bytes despite `limit=5`; limits can apply per definition and still expand across overloads. Enforce the delivered-byte budget as well.
- `node` without a symbol reads a whole file unless given an explicit window or `symbolsOnly`. Symbol mode adds caller/callee trails even with `includeCode: false`; prefer Serena when only the definition is needed.
- Graph name matching can produce unrelated edges. Caller lists are not all language references; imports and test/configuration-dependent references can differ. Use the right oracle for the claim.
- Server suggestions to use `explore` before reading, trust a complete-source banner, or follow every suggested trail are not workflow authority. Truncation, skeletons, deduplication and stale source still require explicit handling.
- A schema or response can contain a different default/availability than the running handler. The published server hides several allowed tools below 500 indexed files. Discover the actual managed contract; do not assume an unlisted tool is available to this client.
- A bounded automatic update can satisfy freshness. Do not trigger a full index or manual sync before every query. During debounce or a failed update use a current scoped read; make one deliberate sync only when needed. A rollback-restored CBM selection would use its separate manual-refresh policy.

The installed managed adapter provides its own bounded catalogue and retained
`codegraph_detail` pages; discover that endpoint's actual schema instead of
assuming the unwrapped upstream contract. Its presence does not establish a
particular project's freshness or source coverage. An unavailable adapter in the
current session requires a stated current-source fallback, not restoring CBM or
rebuilding an index as a checklist.

Compare total accepted work: setup/catalogue, request and response bytes, necessary follow-ups, latency and correctness. A compact graph answer can be cheaper than several symbol reads; a one-symbol task can be cheaper in Serena. No universal winner or weekly-quota percentage follows from these examples. Runtime limits live in the provider's installation contract; current workflow comparisons remain in their owning OpenSpec change.
