//! Single declaration of the repository-relative records that own token-audit
//! finding remediation.
//!
//! The analyzer names one of these routes in every finding, and the native
//! source check verifies that each declared route resolves in current source.
//! Both read this module so a moved or removed owner document fails one
//! declared list instead of drifting per consumer.

/// Runtime context discipline for measured session findings.
pub const TOKEN_WORKFLOW: &str = "docs/token-workflow.md";
/// Delegation and workload-placement guidance for outlier sessions.
pub const AGENT_DELEGATION: &str = "docs/agent-delegation.md";
/// Subscription routing and effort selection for high-effort sessions.
pub const SUBSCRIPTION_MODELS: &str = "docs/subscription-models.md";
/// Tool output bounding owned by the token-efficient-workflow skill.
pub const TOKEN_EFFICIENT_WORKFLOW: &str = ".agents/skills/token-efficient-workflow/SKILL.md";

/// Every documentation route a token-audit finding may name, relative to the
/// repository root. Consumers must verify each route resolves in current
/// source; findings never invent a route outside this list.
pub const TOKEN_AUDIT_OWNERS: &[&str] = &[
    TOKEN_WORKFLOW,
    AGENT_DELEGATION,
    SUBSCRIPTION_MODELS,
    TOKEN_EFFICIENT_WORKFLOW,
];
