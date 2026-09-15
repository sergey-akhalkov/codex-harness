## Context

See proposal.md for motivation. The installed CodeGraph command is the native manager copied into a config-bridge build directory with an adjacent build.json. verify_runtime admits ordinary MCP only when that record is Healthy. Any later edit in the linked checkout makes the same binary source-stale, so mcp codegraph prints that ordinary runtime is disabled and exits before initialize. Codex then logs a closed initialize handshake and omits codegraph. Serena, Graphify and Nuphus do not use this gate.

The 2026-09-12 repair rebuilt that manager. Rebuild restores Healthy until the next source edit, so the failure returns during ordinary pack development. Check currently compares TOML shape and reports connected without asking whether the command can serve. mcp apply-codegraph-registration also goes through verify_runtime, so Check and Install through the registered manager fail the same way after source drift.

## Goals / Non-Goals

**Goals:**
- Keep an already installed, hash-matching CodeGraph adapter able to complete MCP initialize after later checkout edits.
- Keep integrity-checked management available, and keep source-consuming native operations gated on a healthy source match.
- Make Check distinguish cannot-serve from source-stale-but-still-callable.
- Preserve unrelated MCP servers and the no-download ordinary-startup rule.

**Non-Goals:**
- Changing the published CodeGraph 1.6.0 package, resource limits, catalogue or refresh policy.
- Making ordinary Codex startup rebuild native source or auto-activate a new build.
- Relaxing the gate for config localize/overrides or for selecting a stale build as the current source-consuming runtime.
- Replacing the transitional installer in this change.

## Decisions

- Split admission instead of deleting the source-stale gate. Binary hash and identity checks stay. source-stale and source-unavailable remain management-admitted. Serving CodeGraph MCP and launching the owned CodeGraph broker/service from those same recorded binaries become serving-admitted. Config localize/overrides and selected-build resolution stay on a healthy source match so they cannot apply current source through a stale manager.
- Register CodeGraph with a serving-admitted native command. The config-bridge manager may remain that command only when serving is admitted for it. Do not keep a path where Install writes a command that later Codex startups cannot execute after ordinary source edits.
- Route mcp apply-codegraph-registration through management admission, like prepare and retire. Check and recovery must still run after source drift.
- Check inspects serving admission of the registered command without mutation. Missing, altered or serving-refused commands are degraded and not connected. Source-stale with matching binaries may be degraded for rebuild awareness while remaining callable.
- Native tests cover the reproduced handshake: a recorded source-stale manager with matching hashes must complete initialize for mcp codegraph. Check must not report that registration connected when serving is refused. The existing test that currently requires all ordinary MCP runtime to die on source-stale must be narrowed to source-consuming runtime, not CodeGraph serving.

## Risks / Trade-offs

- A source-stale adapter keeps serving until explicit Update. Accepted: ordinary Codex start cannot rebuild; native adapter changes still require Install/Update. Document that limit next to the existing restart boundary.
- Broker launch still used verify_runtime and could fail after the frontend was admitted. Admit the owned CodeGraph service/create path for serving, not only the stdio frontend.
- Consumers might miss that native adapter code changed. Check reports source-stale degraded-but-callable; serving stays up.

## Migration Plan

Install/Update from current source rebuilds and re-registers a serving-admitted command. Existing sessions keep their previous MCP catalogue until restart. If a previous registration already points at hash-matching binaries, a new Codex start should succeed after this change without another rebuild. Rollback is the previous manager gate: source-stale again disables serving.
