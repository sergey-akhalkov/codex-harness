# Native agent configuration preflight

2026-09-09. [agent_config.rs](../../crates/harness-core/src/agent_config.rs)
checks managed role names in base `config.toml` through the native `inventory`
entry point. It parses TOML structure rather than matching declarations inside
comments or instruction strings. Quoted/escaped keys, dotted assignments and
inline tables therefore share the same collision rule.

The dependency is pinned to `toml 0.9.11+spec-1.1.0`, matching the TOML package
in [Codex 0.153.4's pinned lockfile](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/Cargo.lock).
The kit locks its own transitive dependencies; this is not a claim of identical
whole-program builds. Only std/serde/parse features are selected, with the
parser's normal nesting limit retained. The older locally cached TOML 1.0 parser
was replaced before acceptance because native CLI permits TOML 1.1 syntax.

An ordinary base-file snapshot provides exact identity/byte revalidation after
parsing. Input is limited to 1 MiB, malformed/deep data fails privately, and
parser snippets never appear in errors. Missing configuration remains absent;
reparse base configurations are refused. This check covers root agent names,
not complete native configuration semantics or every external profile/resource.
No models, hooks, MCP commands or referenced executables run during preflight.

Passed checks:

- Three core tests cover valid/invalid TOML, quoted/dotted/inline role collisions,
  unrelated roles/settings, comments and multiline strings, size/depth and private errors.
- Two integration tests exercise the actual manager inventory and preservation
  of malformed/linked base configurations. Related installation lock/state tests
  also passed in the combined run.
- An explicit model-free run of the actual upstream executable accepted TOML 1.1
  multiline inline feature settings. Native observation returned Hooks=false,
  Code Mode=true, exit 0 and no remaining job processes; configuration bytes were
  unchanged. This passed in 10.37 seconds under a 512 MiB/50% CPU job.

```text
cargo test -p harness-core --lib agent_config --offline --locked --jobs 1 -- --nocapture
cargo test -p codex-harness --test agent_config --offline --locked --jobs 1 -- --nocapture
cargo test -p codex-harness --test agent_config actual_upstream_and_preflight_accept_toml_1_1_configuration --offline --locked --jobs 1 -- --ignored --nocapture
```

The explicit test requires `HARNESS_NATIVE_CODEX` selecting the original native
executable. Here it was 0.153.4, SHA-256
`444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B`.
Evidence is under
`%LOCALAPPDATA%/codex-harness-evidence/agent-config-68f1e735a80b48aea4c3bc8637c9252e/`;
`native-concurrent-edit.*` preserves an initial compile refusal during a parallel
inventory edit, and `native-fixed.*` the actual successful run. Native receipt
and process evidence are in `%TEMP%/harness-native-feature-discovery-ihawIo/`.

The same parser is reused by [agent descriptor discovery](rust-inventory.md).
Installer orchestration and global activation remain separate unfinished work.
