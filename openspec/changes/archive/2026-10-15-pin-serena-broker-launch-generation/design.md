# Design

## Context

See proposal.md for the failure. The broker already has per-source generation roots and immutable delivered binaries. The client caches a digest but resolves its executable again on each launch; installation links are mutable.

## Goals / Non-Goals

Keep the existing generation lifecycle and request recovery behavior. No new service, retry loop, language backend or automatic migration between client generations is needed.

## Decisions

Resolve the manager path once when constructing the client. Hash that exact path into its source identity and retain it for both ordinary and relocated launches. The service verifies its own executable as before. Refreshing only the cached digest would silently move a live conversation to another build and bypass the existing generation separation; it is unnecessary.

Use the existing real Serena stdio integration fixture. Start through an owned symlink, retarget it to a distinct already built manager supplied by `HARNESS_TEST_NEXT_MANAGER`, retire the owned broker, then query the same project in the original conversation. Also start a new conversation through the updated link and verify both generations remain usable. CI builds the second manager in release mode; local runs can reuse an immutable installed build. This avoids manufacturing modified executable copies for acceptance.

## Risks / Trade-offs

Retained sessions require their immutable build to remain available; this is already the installation lifecycle's ownership model. Changing external registry contents is a separate configuration event and is not masked by this correction.

## Migration Plan

Use normal deploy and its candidate/installed semantic checks. Existing loaded clients predate the fix; restore their exact still-valid broker generation if needed, and use the corrected manager for new sessions. Normal build recovery remains available.
