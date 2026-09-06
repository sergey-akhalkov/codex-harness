# Codebase Memory: shared update and project isolation

Observed on 2026-09-06 with the adopted npm installation of Codebase Memory
0.10.8. The lifecycle updated 0.10.5 after checking the official npm integrity
and native release checksum, a staged real index/query, and a second index/query
after promotion. The exact prior package directory is retained under the
machine-local dependency rollback directory, including local wrapper comments.

[mcp-codebase.py](../../tests/mcp-codebase.py) then indexed two disposable Python
projects with the same directory name, including spaces and Cyrillic. They have
an identical `shared_symbol` and different `only_alpha` / `only_beta` functions.
The actual server assigned distinct project identities. Structured graph queries
returned the selected project's shared and unique functions without the other
project's function. A fresh backend discovered both saved indexes without another
index call. Fifteen tools were exposed. This is actual indexing and querying, not
a handshake-only check.

The test gives the backend an isolated consumer home under
`CODEX_HOME/harness/verification` and terminates only its own observed child
processes before removing its fixture. It also sets the upstream-supported
`CBM_RUNTIME_DIR` and `CBM_CACHE_DIR`: HOME alone does not isolate Windows
rendezvous. The corrected actual test passed indexing, isolated queries and
restart with this explicit private namespace. The host's shared TEMP directory is not
suitable for this backend's private IPC cache: 0.10.8 correctly rejected a mutation
ACL granted there to another Windows identity. No shared TEMP permissions were
changed. Initial failed fixture cleanup left one disposable directory after a
file lock; it does not contain a production index.

This evidence covers task 3.2's package/operations and isolation. Global native
Codex activation, cross-consumer checks, and the final dependency lifecycle remain
separate acceptance obligations.
