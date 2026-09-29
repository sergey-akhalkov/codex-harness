# Bounded code retrieval

Choose the smallest operation that answers the question. Check the active
project and relevant language support once, then reuse it while unchanged.
Serena is the managed semantic surface; scoped native `rg` owns literal text,
regex, configuration and document search. `search_for_pattern` is not part of
the managed Serena catalogue.

| Need | Start with | Initial scope |
| --- | --- | --- |
| File structure | Serena `get_symbols_overview` | One file, `depth: 0`, `max_answer_chars: 4000` |
| Exact symbol in a known file | Serena `find_symbol` | `relative_path`, exact name, `depth: 0`, `max_matches: 1`, `include_body: false`, `max_answer_chars: 4000` |
| Read one function | Serena `find_symbol` | Same scope, `include_body: true`; expand the answer limit only if that body is needed |
| Signature or docstring only | Serena `find_symbol` | Same scope, `include_info: true`, `include_body: false`; a smaller answer than a full body |
| Name variants in known scope | Serena `find_symbol` | `substring_matching: true` with `max_matches` and `max_answer_chars`; expand only on a stated missing fact |
| Exact references / edit safety | Serena `find_referencing_symbols`, `find_implementations`, `find_declaration` | One symbol/file, `max_answer_chars: 4000`; a too-long answer requires narrowing or deliberate bounded expansion |
| Post-edit symbol diagnostics | Serena `get_diagnostics_for_symbol` | Changed symbol, optional `check_symbol_references` for direct referencers, explicit `min_severity`, `max_answer_chars: 4000`; the same freshness caveat as file diagnostics |
| Unknown location | Scoped `rg` for distinctive identifiers, then Serena overview of the hit file | File or directory glob plus bounded matches; derive candidate names from required behavior instead of inventing semantic search |
| Replace one symbol | Serena `replace_symbol_body` | Retrieve the current body first; write the complete replacement; keep project-native checks |
| Insert a sibling symbol | Serena `insert_before_symbol` / `insert_after_symbol` | One unambiguous anchor symbol |
| Rename across references | Serena `rename_symbol` | One unambiguous symbol; verify callers after |
| Remove a symbol safely | Serena `safe_delete_symbol` | One unambiguous symbol; check the reported reference set |
| Matching narrow multi-file text edit | Serena `replace_in_files` | Explicit paths, literal pattern; run `dry_run` first (or set an `expected_count` guard), then apply with the selected `occurrence_ids`; prefer symbol edits when names are known |
| Literal text, config, documents, unsupported source | Scoped `rg` / native read | File or directory and bounded matches/lines |

Batch independent Serena reads into one tool block when they share no
ordering dependency; activation remains once per project session.

For diff impact, establish the exact base and whether the input includes
staged, unstaged or untracked work. Map changed code to unambiguous symbols
before reference queries; a symbol index does not define the Git diff. Review
changed configuration and dynamic consumers with current source. Select a
caller/entry-point check that would fail if the changed contract broke; a
reference list alone does not validate the change.

For architecture, start with a named subsystem/file and its overview, then
follow exact references one hop at a time. For value tracing, inspect the
relevant assignments/arguments/returns in current source. Name matching is
not semantic search; when the language server does not support a file,
use scoped literal search on candidate names and check the resulting bodies.
Report that coverage limit instead of a negative claim about the
implementation.

Serena's `initial_instructions` can advise trusting a refactor without
checks. Keep the consuming project's applicable behavioral checks: tool
success proves the operation's reported execution, not correct caller
behavior. Empty diagnostics remain unverified unless their current complete
analysis is independently known, and the first answer after an external file
edit can be a stale snapshot; retry briefly instead of treating one repeated
or empty result as fresh. Likewise, a mode called `planning` is not an
enforced read-only boundary. Qualify the effective catalogue and rejected
owned writes of the chosen MCP restriction; native tools still retain the
user's separately authorized capabilities. Existing clients may keep an older
catalogue after configuration changes; distinguish a fresh consumer's
discovery from an in-turn change.

For a small question, start with 4 KiB per delivered answer and 8 KiB
cumulative retrieval. Explain the missing fact before increasing breadth;
deliberate responses remain at most 16 KiB. These are output budgets, not
tokenizer or subscription measurements. Use Code Mode to retain the raw
result and return a bounded projection. Check all settled outcomes, protocol
errors and `isError`; preserve warnings, project identity, omitted counts
when known and a detail locator. Do not present a cut source body or list as
complete. An expired locator is an explicit limitation, not permission to
rerun silently.

Compare total accepted work: request and response bytes, necessary
follow-ups, latency and correctness. A symbol edit can be cheaper and safer
than line surgery; a scoped `rg` match list can be cheaper than a broad
symbol query. No universal winner or quota percentage follows from these
examples. Runtime limits live in the provider's installation contract.
