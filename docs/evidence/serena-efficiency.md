# Serena 1.7.0 diagnostics and edit qualification

Observed on 2026-09-08 against installed serena-agent **1.7.0** through the existing
project-scoped broker and stdio proxy. [tests/serena-efficiency.py](../../tests/serena-efficiency.py)
passed the final owned native check in 23.296 seconds. Shared sources
[serena_broker.py](../../tools/code-tools/serena_broker.py),
[serena_proxy.py](../../tools/code-tools/serena_proxy.py) and
[tests/serena-shared.py](../../tests/serena-shared.py) were reused and not modified.
No model probe, vendor patch, or ENABLE_DIAGNOSTICS flip was used.

## Execution identity

| Field | Value |
| --- | --- |
| Command | adopted Serena Python `-B tests/serena-efficiency.py -v` |
| cwd | repository root |
| Knowledge | final check consumes the installed global registry, with its retained explicit Python dependency; no language overlay |
| Serena | 1.7.0, adopted uv tool Python |
| Project languages | owned fixtures declared `language_servers: [python]` |
| Backend | existing basedpyright 1.39.10 cache; language_backend remains LSP |
| Transport | owned CODEX_HOME / SERENA_HOME / HARNESS_SERENA_BROKER_DIR, actual installed `code-tools.json` |
| Fixtures | `%TEMP%/serena-efficiency-ru4yygz3/report.json` (alpha/beta Python projects outside this checkout) |
| Host config | shared Serena config, live registry and broker/proxy source hashes unchanged |

Installed 1.7.0 hashes recorded by the run:

- `serena/tools/tools_base.py` SHA-256 `c6547239c9bba58a761add6f9871fe3ca8d97a6043a2013e3476873cf919e2c5`
- `serena/resources/config/contexts/codex.yml` SHA-256 `cfe3c9221915f0c2a874404b866211402110c216bcea534a29ff73d008afc03e`
- `solidlsp/ls.py` SHA-256 `890c5a689b95ad7ece7c21226040013325e2df877a7ca730e4376e0dd77eba31`

## Exposed surface

Codex context tools included symbolic navigation, `get_diagnostics_for_file`,
`replace_symbol_body`, `replace_in_files`, `insert_before_symbol`,
`insert_after_symbol`, `rename_symbol` and `safe_delete_symbol`.
`create_text_file`, `replace_content`, `read_file`, `execute_shell_command`,
`find_file` and `list_dir` were absent. A direct `create_text_file` call
returned `Unknown tool: create_text_file` and created no file.

`EditingToolWithDiagnostics.ENABLE_DIAGNOSTICS` is `False`. A successful
`replace_symbol_body` that introduced a return-type error returned `OK` with no
inline diagnostic payload.

## Diagnostics

On the owned Python fixture, explicit `get_diagnostics_for_file` after the bad
symbol body returned current basedpyright `reportReturnType` grouped under
`sample.py -> Error -> oracle_value`. With `max_answer_chars=40` that result
was replaced by `The answer is too long (321 characters)...`; a later unbounded
call returned the full grouped object. After correcting the body, the tool
returned `{}`. That empty object is recorded as
`explicit-empty-unauthoritative`, not as a clean LSP result: installed
`request_text_document_diagnostics` can fall back to nonempty published
diagnostics and return `[]` when no accepted payload is obtained.

Writing `external.py` outside Serena still produced a current
`reportReturnType` on that file after the next diagnostic call. The
`sample.py` result stayed `{}`. Installed
`LanguageServerFileChangeNotifier` still walks every tracked source file and
notifies every project language server, so a single-file argument does not
satisfy the strict automatic-work boundary.

## Edits, isolation, reuse

`replace_in_files` no-op and `expected_count` mismatch both reported error and
left bytes unchanged. A matching replacement, `insert_after_symbol` and
`safe_delete_symbol` succeeded. `find_referencing_symbols` found both the import
and invocation in `caller.py`. Two proxy clients on the same alpha root reused
one worker; activating beta created a separate worker while alpha retained its
original one. Same-name `oracle_value` bodies stayed isolated (`return 1` vs
`return 2`). Cold connect plus overview took 4.578s; a second client on the
warm worker took 0.336s.

## Oracle comparison

Fixed oracle: replace `oracle_value` so it returns `42`, then run
`from sample import oracle_value; raise SystemExit(0 if oracle_value() == 42 else 1)`
with the adopted Serena Python. Both arms used that same expected-edit check.
Serena arm: `find_symbol` + `replace_symbol_body` + native Python. Native arm:
AST-confirmed body rewrite + the same native Python. Two warm pairs, then stop:

| Pair | Serena (s) | Native (s) | Winner | Material vs 10% and 0.2s |
| --- | ---: | ---: | --- | --- |
| 1 | 0.261 | 0.040 | native | yes |
| 2 | 0.263 | 0.040 | native | yes |

Repeat spread 0.002s. Means 0.262s vs 0.040s. The 0.222s gap exceeds the
0.202s threshold including spread and the 10% threshold. Native wins this
tiny rewrite, so it remains the appropriate path for that case. Both arms used
zero model calls, so parent/child model usage was zero; this is not a model
effort or weekly-quota comparison. Cold process start remains a separate cost.

Earlier qualification used an owned registry overlay and found inconclusive
timing after repeat spread (`serena-efficiency-pfszu5fm`). Integration then exposed
that retiring every dependency row also disabled useful explicit Python. The
final run follows that correction and the added cross-file reference check;
it is not an extra attempt to obtain a preferred timing result.

## Selected scope

Automatic Serena diagnostics stay **rejected**. Explicit Python navigation and
suitable symbol/`replace_in_files` edits remain usable with project-native
checks. Their useful compact cross-file results and verified edits are also
demonstrated in the [existing consumer assessment](mcp-tool-selection.md).
Retention is for those explicit needs; this comparison rejects mandatory Serena
use for tiny rewrites and establishes no general speed advantage. The separate
diagnostic carrier adds no demonstrated need to that workflow. Creation and
other Codex-excluded tools keep native paths. harness-lsp
was not used. An empty diagnostic object must not be reported as clean.

## Limits

This is Python-only, using the declared project language list and retained
verified Python discovery row. No
claim is made for TypeScript or other languages from this run. The comparison
does not include model setup, review or recovery beyond the deterministic
oracle.
