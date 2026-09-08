## Why

The user wants subscription-backed models from other providers in the official Codex CLI and deterministic model assignments for individual subagent roles. Their SuperGrok Heavy account already works in OpenCode; the kit must deliver equivalent access globally without disrupting that installation.

## What Changes

- Connect a version-pinned OpenCodex loopback proxy and its Windows background lifecycle through the kit installer.
- Deliver reusable non-secret proxy configuration by direct link; keep OAuth credentials, logs, catalog caches and ownership records on the host.
- Authenticate xAI with the user's subscription, preserve ChatGPT access for the main GPT-6 Astra agent, and expose the account's actual Grok model identifiers in Codex.
- Add a globally discoverable `grok_reviewer` role with a fixed provider/model and compatible reasoning. Document how to assign another enabled provider/model to another role.
- Use Codex multi-agent v1 for heterogeneous delegation; prevent silent fallback away from the role's selected model.
- Verify actual CLI and subagent calls outside this checkout, failure behavior, restart and rollback, and coexistence with OpenCode and the existing harness.

## Capabilities

### New Capabilities

- `subscription-model-routing`: Global subscription authentication, provider routing, exact named-role model selection and reversible proxy delivery.

### Modified Capabilities

None. The new capability extends the existing installation lifecycle without weakening the linked-global-kit contract.

## Impact

Windows installation orchestration and new focused proxy lifecycle support; linked non-secret OpenCodex settings; global agent definitions; documentation and proportionate lifecycle/native consumer checks. OpenCodex 2.44.0 (source commit 07b48da8fd63881e848d26e0bd50087864f5573e) is the researched candidate. Codex remains the official installed CLI. Authentication requires the user's xAI account authorization; other subscriptions are supported through documented extension points, with no claim that untested providers have been activated.
