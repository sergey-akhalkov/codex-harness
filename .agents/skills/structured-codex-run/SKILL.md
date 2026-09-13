---
name: structured-codex-run
description: Run bounded read-only Codex inspections with schema and independent correctness checks for repeated or machine-consumed results. Skip ordinary interactive work.
---

# Structured Codex run

Define the working directory, relevant revision and dirty inputs, allowed effects, model/provider/subscription route, expected result, independent oracle and time/output limits before execution. Use the user's approved routing policy; do not silently switch providers or billing. Parseable JSON alone does not establish schema enforcement on an external route. Respect any model-family constraints in current instructions.

Check the installed `codex exec --help` contract. Native `--output-schema` constrains the final message, `--json` emits event JSONL, and `--output-last-message` writes the final result separately. Pass prompt text over stdin and arguments as separate values. Choose the sandbox explicitly; inherited Full Access is inappropriate for a read-only inspection. These semantics are documented in [non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode); actual route support still needs a native consumer check.

For the bundled contract, use [inspection.schema.json](assets/inspection.schema.json) and the existing [helper](scripts/run.py); read [the invocation contract](references/invocation.md) for its prerequisites, native alternative, limits and evidence files. The helper uses read-only execution and private evidence without installing or reconfiguring anything. Invoke a model only within the user's authorized route and conversation-visibility policy.

Prepare the oracle outside model-writable sources: compute correctness from independent expected facts rather than the model's claims. Record relevant input files, including the oracle's inputs. Restrict MCP/external tools to allowed read-only effects; the shell sandbox does not control external-account authority.

Accept only natural exit zero, a successful terminal task event, fresh schema-valid final JSON, no unresolved issues, unchanged recorded inputs, and a successful independent oracle. Timeouts, forced termination and output-limit stops retain plausible output as partial evidence only. Missing, malformed, stale, wrong or incomplete results remain failures. Inspect the original error and retained evidence before a bounded retry; do not rewrite a receipt or weaken the expected answer to produce success.
