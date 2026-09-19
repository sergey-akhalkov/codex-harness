# Routing examples

Read this when deciding whether to stage a candidate.

| Observation | Route |
| --- | --- |
| Reusable procedure with a checkable result | Stage a create/update candidate |
| Project-specific fact or command already in a record | Update the owning record |
| Tentative idea | Keep tentative; do not activate |
| Existing tool or documented entry point | Do not invent a skill |
| Read-only or routine success | No-op |

`skill-creator` writes only to the staging path you pass. Never point it at `.agents/skills` or the user global link slot.
