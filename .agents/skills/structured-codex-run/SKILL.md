---
name: structured-codex-run
description: Run bounded read-only Codex inspections with native output-schema, separate process and event evidence, and an independent correctness check. Use for repeated or machine-consumed results, not ordinary interactive work.
---

# Structured Codex run

Define the working directory, relevant revision and dirty inputs, allowed effects, model/provider/subscription route, expected result, independent oracle and time/output limits before execution. Use the user's approved routing policy; do not silently switch providers or billing. Parseable JSON alone does not establish schema enforcement on an external route. Respect any model-family constraints in current instructions.

Check the installed `codex exec --help` contract. Native `--output-schema` constrains the final message, `--json` emits event JSONL, and `--output-last-message` writes the final result separately. Pass prompt text over stdin and arguments as separate values. Choose the sandbox explicitly; inherited Full Access is inappropriate for a read-only inspection. These semantics are documented in [non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode); actual route support still needs a native consumer check.

For the bundled bounded inspection contract, use [inspection.schema.json](assets/inspection.schema.json) and [run.py](scripts/run.py). The helper requires Windows, PowerShell 7.4+, Python 3.10+ and the linked kit's existing `reproduce-regression` process observer. It creates a unique evidence directory outside the target, passes a per-run identity in the prompt, fixes the sandbox to read-only, and retains final JSON, event JSONL, stderr, process receipt and acceptance separately. It does not enable features, install models or change global configuration. A run spends model quota only when explicitly invoked.

Read [the invocation contract](references/invocation.md) when using the helper. Prepare the oracle outside model-writable sources: it must compute correctness from independent expected facts, not trust the model's status or repeat its claims. Record the relevant input files; include the oracle's own inputs. Review and restrict available MCP/external tools to the allowed read-only effects; the shell sandbox does not grant or revoke external-account authority.

Accept only natural exit zero, a successful terminal task event, fresh schema-valid final JSON, no unresolved issues, unchanged recorded inputs, and a successful independent oracle. Timeouts, forced termination and output-limit stops retain plausible output as partial evidence only. Missing, malformed, stale, wrong or incomplete results remain failures. Inspect the original error and retained evidence before a bounded retry; do not rewrite a receipt or weaken the expected answer to produce success.
