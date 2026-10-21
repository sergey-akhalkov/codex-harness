## Context

The native discovery catalogue selects Python, Rust, Pascal and C++. Its existing candidate selection handles absent and ambiguous installations. Serena accepts a caller-pinned launch command, but Roslyn initializes its dependency provider first. That provider must also resolve the adopted host and DLL without installation.

## Goals / Non-Goals

Deliver semantic navigation on an existing C# solution with the existing project SDK. Do not change automatic diagnostics, provision packages during sessions, install another runtime or restore unrelated languages.

## Decisions

Adopt an unambiguous versioned Roslyn cache only when the expected server DLL, matching dependency metadata and runtime configuration are present, together with a compatible installed .NET host on the explicit discovery PATH. Pin that host, DLL and stdio arguments in the existing registry. Discovery remains read-only and never executes package code.

Reuse the existing conditional/reuse-only lifecycle, ambiguity handling and bounded file observations. Package metadata and hashes are observations, not upstream integrity or semantic readiness. A real connected Serena call establishes navigation; explicit diagnostics retain their freshness and completeness limits.

Prepend the validated adopted .NET directory to the worker PATH, preserving the inherited entries and existing Pascal support. Set Roslyn's supported runtime-dependency override to the absolute adopted DLL and its recorded server version. The provider then reuses the same files before the explicit launch command runs. A direct installed-provider check with downloads refused confirmed this path; the managed real-solution check remains the acceptance consumer.

Forward the existing `APPDATA` value into the broker alongside its other explicit Windows environment inputs. Without it, the installed .NET SDK cannot resolve existing NuGet assets and reports a null path while loading the lock file. A native `ResolveReferences` check reproduced the failure with only `APPDATA` removed and passed with it present. No package restore or product binary replacement is needed.

A bounded predeployment call to the existing Serena package, using the generated configuration and the broker's minimal environment plus `APPDATA`, loaded the real C# project, read a method and found its cross-file callers. Explicit file diagnostics contained only a style suggestion; missing dependency/type errors disappeared. This precheck does not replace final verification through the installed managed entry point.

## Risks / Trade-offs

Support is bounded to the installed Roslyn net10.0 Windows x64 layout and compatible .NET 10 runtimes. Unsupported layouts remain unavailable. Existing multiple server versions remain ambiguous rather than selecting an arbitrary version. A runtime file inventory cannot establish successful startup, so actual solution acceptance follows lifecycle delivery.

The adopted Roslyn package declares MIT licensing and upstream Microsoft ownership. Existing installation fingerprints and dependency metadata are checked locally; independent upstream package integrity remains unknown. No new dependency is acquired by this change.
