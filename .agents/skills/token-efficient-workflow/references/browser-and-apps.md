# Browser sequences and connected resources

## Nuphus browser route

List tabs and establish the intended URL/title before interaction. Use an owned
test page for acceptance writes. Start with `browser_snapshot` scoped by
`selector` to the relevant form or region. Use its current element references;
after navigation or replacement, obtain a fresh snapshot. Keep screenshots for
visual questions and DOM observations for values and counts.

Inspect the current `browser_exec` contract. In the qualified helper interface,
await every dependent operation, for example:

```javascript
await h.fill('#quantity', '3');
await h.click('#apply');
```

JavaScript here is the browser tool's required interface. This is not a separate
automation service. Batch only understood actions up to the next semantic
checkpoint. Inspect every returned step's `success` and `detail`; a fulfilled
MCP response can contain a failed step. An empty array does not prove execution.
Observe the intended DOM effect separately, including counters or other
exactly-once evidence when duplication matters.

After a partial failure, inspect the same page before choosing the next action.
If Apply succeeded and Finish did not, do only Finish after confirming the
applied value/count. Do not rerun the entire sequence or retry a write merely
because a response was empty. Stop dependent effects when their state is unknown.
Keep unrelated tabs/windows intact and restore any changed selection after
owned inspection; use actual tab-management capabilities rather than closing a
shared browser to remove one test tab.

## Connected App route

Use a matching connected App for remote repository, PR, CI or document reads.
Discover only the needed contract. Resolve the intended repository and ref, or
the connected document session and supported schema, before retrieving content.
Keep the canonical resource identity in the selected result; same names do not
establish the same resource. Use bounded fields and retained details for a large
response. GitHub release reads should preserve the requested tag and exact asset
identity rather than substituting the latest release.

Check availability with the intended read. A denied/missing resource or absent
document session is an explicit negative result; neither a connector listing nor
an unrelated successful read proves access to it. Use an authorized source or
native fallback and state the limitation. Do not install an App, log in, change
permissions or perform external writes solely to make a read succeed. Document
Control operations require the actual connected session/schema, not a guessed
document path.
