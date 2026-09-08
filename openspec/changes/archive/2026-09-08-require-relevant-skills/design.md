## Context

The global instruction source is already linked and contains the accepted MCP policy. Skills have scope descriptions and can restrict implicit invocation.

## Goals / Non-Goals

Make applicable skill workflows mandatory with one concise policy. Preserve existing skill files, invocation controls and user authorization.

## Decisions

Add a skill-selection section adjacent to MCP selection and replace the previous optional-sounding sentence with a reference to this rule. Require actual workflow use and reconsideration on context changes. Native prompt-input checks exercise the existing global loading entry point without another model request.

## Risks / Trade-offs

Overactivation can waste context or trigger the wrong workflow. Match actual scope and invocation conditions, use complementary workflows only, and load supporting references progressively. Instructions guide behavior; loading evidence does not prove universal model compliance.

## Migration Plan

The existing AGENTS.md link delivers source edits to new sessions. Preserve the prior MCP policy. Rollback removes only the new section and restores the single changed context sentence, using the local pre-change backup if needed.
