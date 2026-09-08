# Global MCP selection: live acceptance

Date: 2026-09-07. User request: test all MCP integrations exposed in this session,
prefer useful capabilities globally, and verify subagent access.

## Scope and observed results

Five local servers were exposed: Serena, Codebase Memory, Graphify, harness-lsp
and Nuphus. Apps additionally exposed GitHub and Document Control. The checks used
actual exposed MCP calls, not only registration lists or standalone STDIO clients.
This is representative integration coverage, not testing every endpoint or every
supported language. No external writes, package changes or service restarts occurred.

| Integration | Parent evidence | Practical assessment and limits |
| --- | --- | --- |
| Serena 1.7.0 | Active `codex-harness`, Python LSP ready; overview returned four functions from `mcp_provision.py`. In an owned outside Python fixture, `find_symbol` read `normalize`, references identified `greeting`, and `replace_symbol_body` changed lowercasing to uppercasing. Executing the caller returned `HELLO`. | Compact, useful symbol reads and semantic edits. The active harness project advertised Python; this does not prove every language is enabled there. Onboarding was absent but navigation worked. |
| Codebase Memory 0.10.8 | Initial project listing had six roots and no harness. `index_repository` created its index. Exact graph search found `provision` at lines 47–113; an inbound trace found `main`, with LSP evidence, and a snippet exposed its actual call. | Useful repository relationships. Initial fast indexing excluded `tools`; later coverage reported a full generation and the exact source query succeeded. Coverage reported partial PowerShell parsing and `metadata_changed`; no completeness/freshness claim was made from that signal. |
| Graphify | Default graph returned 88,513 nodes and 172,936 edges. An explicit outside fixture returned its two known nodes and the expected `calls` edge. An explicit directory without a graph returned `isError=true` and `graph.json not found`. | Useful for an existing relevant graph. The default graph is not automatically the current repository. The synthetic graph proves selection/query behavior, not automatic graph extraction. |
| harness-lsp | Outside Python fixture: symbols returned `normalize` and `greeting`; diagnostics found `reportReturnType` for returning a string as `int`; after correction it returned `clean`, no diagnostics, and a new revision with `versioned-push` freshness. | Useful fresh diagnostics and explicit workspace navigation; richer output than Serena's compact overview. |
| Nuphus | Desktop resolution returned 2520 × 1680. An owned `about:blank` browser page received an input/button fixture; snapshot exposed controls, typing and clicking produced `mcp-verified`, confirmed by DOM evaluation. `browser_close` succeeded. | Useful browser actions with inspectable effects. The first click correctly reported no effect after 15 seconds: the fixture's inline handler had malformed quoting. Assigning a proper handler corrected the fixture and the same click succeeded. Desktop input, OCR and user windows were not exercised here. |
| GitHub App | `get_user_login` returned the authenticated account in parent and child; parent `get_repo` returned `openai/codex` metadata and read permission. | Authenticated reads and public repository metadata work; private-repository access and repository/PR mutations were not exercised. Prefer structured connector operations when matching the task. |
| Document Control App | `list_document_sessions` returned `executors: []`, without a transport error, in parent and child. | Discovery works; document reads/edits cannot be evaluated without a connected session. No universal document-tool preference is claimed. |

Serena and Codebase Memory already carried selection guidance in their tool
descriptions. The previous portable startup principles did not name a concrete
MCP selection workflow. Their non-use cannot be attributed solely to failed
settings: both answered directly. A missing harness index was a separate setup
gap, now initialized. Qualitative usefulness comes from the compact results and
verified effects above; no comparative token benchmark or quota-saving percentage
was measured. Bulk tool-description dumps can themselves waste context.

## Subagent evidence

Named `middle` child Bernoulli, id `01a07ce1-524e-7a83-9ea3-7e98e847da78`, ran with
`fork_context=false` on the configured Grok subscription. Its transcript records
direct calls to all five local MCPs and both Apps: Serena instructions/overview;
Codebase Memory project listing, exact `provision` search and coverage; Graphify
query of the same two-node outside fixture; harness-lsp symbol navigation using
its own session id; Nuphus desktop resolution; GitHub login; Document Control
session discovery. No named tool was missing. Parent and child observed the same
expected symbols, graph edge and connector state. This establishes actual child
access on this host, not availability in clients with different tool policies.

## Global source and activation

The user explicitly authorized changing the global instructions. The source is
[the MCP section](../../global/principles-of-work.md#mcp-tool-selection);
`C:\Users\noilw\.codex\AGENTS.md` remains its existing symbolic link. No duplicated
instruction body or replacement config is installed. New sessions load the section;
already running contexts may require a new session. Each child must verify its own
project context, and native instruction precedence remains applicable.

Codex CLI 0.153.4 was observed. The official
[MCP contract](https://learn.chatgpt.com/docs/extend/mcp#supported-mcp-features)
documents server-wide initialization instructions alongside tools. The global
section adds cross-server selection; it is guidance, not enforced scheduling.

Native `codex -C <outside-directory> debug prompt-input` passed from both the
outside fixture Git repository and its separate `second-project` Git repository.
Each parsed prompt contained the full updated global source, including the new
section. The second also contained its local `AGENTS.md` marker. The linked file
and source SHA256 matched. No MCP configuration, profile or instruction override
was supplied to these checks, and no model request was needed.

`openspec validate prefer-verified-global-mcp --strict` and scoped
`git diff --check` passed. Eight documents passed checks of 59 local link targets
and the new MCP anchors. Explicit current Markdown diagnostics returned `clean`
for the global source, project decisions, usage guide and acceptance report;
automatic diagnostics also reported the four change artifacts clean. The usage
guide has one source-state heading. All seven change tasks are complete.
Unrelated pre-existing LSP diagnostics do not establish a defect in this
instruction change.

## Local evidence and recovery

The owned outside fixture and pre-change instruction backup are under TEMP
`harness-mcp-routing-7765f8171af94add821625e0a756f729`. Parent session id:
`01a07cdf-967d-72f0-9271-ffacbc414227`. Child transcript has the id above.
These machine-local artifacts are outside tracked reusable configuration.

Rollback: remove only the added `MCP tool selection` section from the source,
preserving later unrelated edits; compare with `principles-before.md` when needed.
The global link remains intact. No MCP shutdown or subscription proxy restart is
needed. Serena was returned to `codex-harness`; the owned browser was closed.

Archive audit 2026-09-08: current source and global link hashes matched; native prompt-input from both external repositories loaded the complete source and the second local AGENTS.md. Primary receipts are retained in TEMP harness-spec-audit-8ae152f990de442da00b60c3a6fc9466. Main requirements were synchronized and strict validation passed before archival. The first audit assertion expected an absent phrase; it was corrected to compare the complete source without changing product instructions.
