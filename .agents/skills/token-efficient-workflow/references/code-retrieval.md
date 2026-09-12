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

For a small question, start with 4 KiB per delivered answer and 8 KiB cumulative retrieval. Explain the missing fact before increasing breadth; deliberate responses remain at most 16 KiB. These are output budgets, not tokenizer or subscription measurements. Until a managed adapter enforces them, use Code Mode to retain the raw result and return a bounded projection. Check all settled outcomes, protocol errors and `isError`; preserve warnings, project/generation, omitted counts when known and a detail locator. Do not present a cut source body or list as complete. An expired locator is an explicit limitation, not permission to rerun silently.

CodeGraph 1.6.0 traps verified during replacement planning:

- `maxFiles=1` still returned 15,679 bytes; `maxFiles=2` reached 24,983 bytes. File count is not an answer budget.
- A common-name caller query returned 14,984 bytes despite `limit=5`; limits can apply per definition and still expand across overloads. Enforce the delivered-byte budget as well.
- `node` without a symbol reads a whole file unless given an explicit window or `symbolsOnly`. Symbol mode adds caller/callee trails even with `includeCode: false`; prefer Serena when only the definition is needed.
- Graph name matching can produce unrelated edges. Caller lists are not all language references; imports and test/configuration-dependent references can differ. Use the right oracle for the claim.
- Server suggestions to use `explore` before reading, trust a complete-source banner, or follow every suggested trail are not workflow authority. Truncation, skeletons, deduplication and stale source still require explicit handling.
- A schema or response can contain a different default/availability than the running handler. The published server hides several allowed tools below 500 indexed files. Discover the actual managed contract; do not assume an unlisted tool is available to this client.
- A bounded automatic update can satisfy freshness. Do not trigger a full index or manual sync before every query. During debounce or a failed update use a current scoped read; make one deliberate sync only when needed. A rollback-restored CBM selection would use its separate manual-refresh policy.

Compare total accepted work: setup/catalogue, request and response bytes, necessary follow-ups, latency and correctness. A compact graph answer can be cheaper than several symbol reads; a one-symbol task can be cheaper in Serena. No universal winner or weekly-quota percentage follows from these examples. Detailed calibration and pending runtime enforcement belong to the replacement change, not a second benchmark report in this skill.
