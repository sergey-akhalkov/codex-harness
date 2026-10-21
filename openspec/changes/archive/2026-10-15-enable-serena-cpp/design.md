## Context

The managed gateway already pins adopted native language servers and bounds their processes. C++ is a retired catalogue row; project detection incorrectly asks for `clangd` instead of Serena's `cpp` identifier.

## Goals / Non-Goals

Restore C++ navigation using an existing shared installation and the project's real compilation database. Do not introduce another server, runtime downloader, compiler, or automatic diagnostics.

## Decisions

- Reuse versioned `clangd_<version>/bin/clangd.exe` installations in the existing OpenCode shared cache. Record executable fingerprints and distinguish directory version labels from verified runtime identity. Ambiguous or missing installations fail closed through existing discovery selection.
- Keep the same native launch settings and broker lifecycle. Correct project marker identifiers rather than adding an alias for an invalid Serena language.
- Extend the existing semantic acceptance consumer with C++ symbol, reference and disposable edit checks. Project-specific compiler flags and native checks remain necessary outside a header-free fixture.
- Keep C++ reuse-only in dependency planning, including when newer releases exist. No dependency installation or upgrade is needed.

## Risks / Trade-offs

An installed binary or versioned directory alone does not prove usable navigation or upstream integrity: require actual managed semantic acceptance. Project headers and flags may be incomplete: configure a real compilation database and verify a translation unit before declaring the consuming project ready. Missing or multiple cache versions stay explicit and do not trigger a download.

## Migration Plan

Deliver a verified immutable manager through `deploy`, apply `install --code-tools-only`, and restart the consuming MCP session. Existing language selection and unrelated shared packages remain preserved.
