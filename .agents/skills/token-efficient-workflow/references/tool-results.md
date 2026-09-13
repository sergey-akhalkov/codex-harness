# Mixed and oversized tool results

Scope the request first. When its result still needs shaping, retain the original
settled outcome with a unique task-local `store` key before returning a projection.
Keep that key stable for detail reads; never overwrite it with a summary. The
Code Mode store is temporary and is not a durable audit record.

Inspect each outcome before choosing fields:

| Observation | Preserve in the delivered result |
| --- | --- |
| Rejected promise | Operation identity and rejection cause; execution/effects may be unknown |
| Fulfilled MCP response with `isError` | Tool failure and its diagnostic, even when structured data exists |
| Fulfilled process with nonzero exit | Exit code, relevant stderr/output and input/build identity |
| Process still running | Live session/cell handle and incomplete status; wait on that handle |
| Successful data | Fields answering the question plus source/root/freshness and coverage |
| Missing data, empty step list, partial or oversized data | Explicit incomplete/omitted status and a valid detail key or page |

MCP `content` and `structuredContent` often duplicate each other. Prefer the
structured payload when sufficient, otherwise the relevant text/image blocks.
Read diagnostics from either representation before discarding duplicates. A
text block may itself contain JSON; parse that envelope before selecting fields.
Do not print a large nested JSON string merely because its outer object is small.

Return per-operation outcomes, not only an aggregate `ok`. Distinguish a tool
error from an unavailable transport, and preserve unknown effects after either.
If a projection omits records, include the known total and omitted count; if the
total is unknown, say so. A clipped body is not a complete source definition.
Use a bounded field/line/page request against the retained raw result to recover
details. If `load(key)` is absent, report expiry; reassess the need and authority
for another call, especially a mutation. Do not silently replay it.

For example, a release lookup can expose canonical repository URL, requested tag,
the selected asset's name/digest and a detail key. The rest of the release remains
in the original retained response. This is a presentation choice, not a claim
that response bytes equal model tokens or subscription savings.
