## Why

CMake projects are refused by the managed Serena gateway even when a shared clangd installation is available. Restore useful C++ navigation without installing another server or bypassing the managed resource boundary.

## What Changes

- Adopt an existing versioned shared clangd installation as an optional, reuse-only backend.
- Map CMake and Visual C++ project markers to Serena's canonical `cpp` language.
- Exercise C++ through managed semantic acceptance and deliver the configuration through the existing installation lifecycle.

## Capabilities

### Modified Capabilities

- `global-code-tools`: include conditional C++ navigation with explicit compiler inputs and truthful diagnostic limits.

## Impact

Dependency discovery, the language catalogue, managed Serena configuration, semantic acceptance, and the code-tools guide. No backend download, project compiler replacement, automatic diagnostics, or changes to unrelated language selection.
