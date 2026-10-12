## Why

Managed Serena refuses C# projects even when a compatible shared Roslyn server and .NET runtime already exist. This prevents semantic investigation of existing C# integration tools.

## What Changes

- Restore conditional C# navigation by adopting the existing shared Roslyn cache and a compatible existing .NET host.
- Pin the native launch command through the current registry and code-tools lifecycle; keep provisioning disabled during MCP sessions.
- Verify actual C# symbols, references and explicit diagnostic behavior after delivery.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-code-tools`: conditional C# support through a reusable Roslyn installation.

## Impact

Dependency discovery, Serena launch configuration, focused native tests, language selection and its operating guide. No new package, runtime installation, automatic diagnostics or other retired language is required.
