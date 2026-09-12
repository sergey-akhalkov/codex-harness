//! Deterministic classification of native turn errors for dispatch recovery.
//!
//! The input is the "error" object of a native error notification (TurnError):
//! an object with a required "message" and an optional "codexErrorInfo" field
//! whose shape follows the installed app-server schema. Classification reads
//! only that structured field; free-text messages, HTTP status codes and
//! unknown future variants never upgrade a failure to a confirmed cause.
//!
//! The caller keeps the raw notification, so this module adds no reset times,
//! percentages, retry decisions or account state of its own.

use serde::Serialize;
use serde_json::Value;

/// Why a turn failed, projected from structured provider evidence.
///
/// Variants keep confirmed causes separate so that routing can distinguish,
/// native usage-limit exhaustion from throttling and a local session budget.
/// The code alone does not establish the account or reset-window scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureCause {
    /// Native usage limit exhausted; its account/window scope remains separate.
    Quota,
    /// Confirmed temporary request throttling (rateLimitExceeded).
    Throttle,
    /// Local per-session budget exhausted (sessionBudgetExceeded); this is a
    /// client-side budget, not account quota.
    LocalBudget,
    /// Provider capacity is overloaded (serverOverloaded).
    Overloaded,
    /// Credentials or authorization failed (unauthorized).
    Authentication,
    /// The conversation no longer fits the model context (contextWindowExceeded).
    ContextLimit,
    /// A policy layer rejected the work (cyberPolicy, misalignmentPolicyViolation).
    Policy,
    /// The response connection could not be established.
    Transport,
    /// Native retry attempts ended; this does not establish the original cause.
    RetryExhausted,
    /// An internal service error, without a more specific cause.
    Service,
    /// The response stream disconnected mid-turn before completion
    /// (responseStreamDisconnected); the output is incomplete.
    IncompleteOutput,
    /// No structured cause is available. This includes null or missing
    /// codexErrorInfo, generic rejections such as badRequest, and variants
    /// introduced after this classifier was written.
    Unknown,
}

/// Classifies a TurnError-shaped JSON value by its structured codexErrorInfo.
///
/// Unknown, null and future shapes stay [FailureCause::Unknown]. A message
/// that merely mentions quota, or a bare HTTP 429/500 status, is not treated
/// as confirmed quota: only the explicit usageLimitExceeded code is.
pub fn classify(error: &Value) -> FailureCause {
    if !error["message"].is_string() {
        return FailureCause::Unknown;
    }
    let Some(info) = error.get("codexErrorInfo") else {
        return FailureCause::Unknown;
    };
    if let Some(code) = info.as_str() {
        return match code {
            "usageLimitExceeded" => FailureCause::Quota,
            "rateLimitExceeded" => FailureCause::Throttle,
            "sessionBudgetExceeded" => FailureCause::LocalBudget,
            "contextWindowExceeded" => FailureCause::ContextLimit,
            "serverOverloaded" => FailureCause::Overloaded,
            "cyberPolicy" | "misalignmentPolicyViolation" => FailureCause::Policy,
            "unauthorized" => FailureCause::Authentication,
            // Server-side infrastructure failure without a quota or capacity
            // claim; treat as delivery failure, not as a confirmed cause.
            "internalServerError" => FailureCause::Service,
            // badRequest is a generic rejection and must not be read as a
            // model decision; everything else is a future or unmatched code.
            _ => FailureCause::Unknown,
        };
    }
    // Object variants carry exactly one discriminator key; classify by that
    // key while ignoring the optional httpStatusCode, which never upgrades
    // the cause by itself.
    let discriminator = info
        .as_object()
        .filter(|map| map.len() == 1)
        .and_then(|map| map.keys().next())
        .map(String::as_str);
    match discriminator {
        Some("httpConnectionFailed") | Some("responseStreamConnectionFailed") => {
            FailureCause::Transport
        }
        Some("responseStreamDisconnected") => FailureCause::IncompleteOutput,
        Some("responseTooManyFailedAttempts") => FailureCause::RetryExhausted,
        _ => FailureCause::Unknown,
    }
}

/// Correlated terminal failure, retaining the original native error unchanged.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnFailure {
    pub thread_id: String,
    pub turn_id: String,
    pub cause: FailureCause,
    pub error: Value,
}

impl TurnFailure {
    pub fn from_event(event: &Value) -> Option<Self> {
        let params = &event["params"];
        if event["method"] != "turn/completed" || params["turn"]["status"] != "failed" {
            return None;
        }
        let error = params["turn"]["error"].clone();
        Some(Self {
            thread_id: params["threadId"].as_str()?.into(),
            turn_id: params["turn"]["id"].as_str()?.into(),
            cause: classify(&error),
            error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FailureCause, TurnFailure, classify};
    use serde_json::{Value, json};

    fn error_with(info: Value) -> Value {
        json!({ "message": "request failed", "codexErrorInfo": info })
    }

    #[test]
    fn confirmed_quota_throttle_and_local_budget_stay_distinct() {
        assert_eq!(
            classify(&error_with(json!("usageLimitExceeded"))),
            FailureCause::Quota
        );
        assert_eq!(
            classify(&error_with(json!("rateLimitExceeded"))),
            FailureCause::Throttle
        );
        assert_eq!(
            classify(&error_with(json!("sessionBudgetExceeded"))),
            FailureCause::LocalBudget
        );
    }

    #[test]
    fn quota_language_without_structured_cause_is_not_quota() {
        let message_only = json!({
            "message": "You have exceeded your weekly usage quota; try again later."
        });
        assert_eq!(classify(&message_only), FailureCause::Unknown);

        let null_info = json!({
            "message": "usage limit exceeded for this week",
            "codexErrorInfo": null
        });
        assert_eq!(classify(&null_info), FailureCause::Unknown);
    }

    #[test]
    fn http_429_on_a_transport_failure_is_not_throttle_or_quota() {
        let connection_failed = json!({
            "message": "stream failed",
            "codexErrorInfo": {
                "httpConnectionFailed": { "httpStatusCode": 429 }
            }
        });
        assert_eq!(classify(&connection_failed), FailureCause::Transport);

        let stream_connect_failed = json!({
            "message": "stream failed",
            "codexErrorInfo": {
                "responseStreamConnectionFailed": { "httpStatusCode": 500 }
            }
        });
        assert_eq!(classify(&stream_connect_failed), FailureCause::Transport);
    }

    #[test]
    fn internal_server_error_with_quota_wording_stays_service_error() {
        let error = json!({
            "message": "upstream quota gateway failed",
            "codexErrorInfo": "internalServerError"
        });
        assert_eq!(classify(&error), FailureCause::Service);
    }

    #[test]
    fn incomplete_stream_is_distinguishable_from_transport_and_retry_loss() {
        let disconnected = json!({
            "message": "stream ended early",
            "codexErrorInfo": {
                "responseStreamDisconnected": { "httpStatusCode": null }
            }
        });
        assert_eq!(classify(&disconnected), FailureCause::IncompleteOutput);

        let retries_exhausted = json!({
            "message": "gave up",
            "codexErrorInfo": { "responseTooManyFailedAttempts": { "httpStatusCode": 502 } }
        });
        assert_eq!(classify(&retries_exhausted), FailureCause::RetryExhausted);
    }

    #[test]
    fn generic_bad_request_is_not_a_model_rejection_signal() {
        assert_eq!(
            classify(&error_with(json!("badRequest"))),
            FailureCause::Unknown
        );
    }

    #[test]
    fn other_structured_causes_map_to_their_variants() {
        assert_eq!(
            classify(&error_with(json!("contextWindowExceeded"))),
            FailureCause::ContextLimit
        );
        assert_eq!(
            classify(&error_with(json!("serverOverloaded"))),
            FailureCause::Overloaded
        );
        assert_eq!(
            classify(&error_with(json!("unauthorized"))),
            FailureCause::Authentication
        );
        for code in ["cyberPolicy", "misalignmentPolicyViolation"] {
            assert_eq!(classify(&error_with(json!(code))), FailureCause::Policy);
        }
    }

    #[test]
    fn future_and_unmatched_shapes_stay_unknown() {
        assert_eq!(
            classify(&error_with(json!("brandNewErrorCode"))),
            FailureCause::Unknown
        );
        assert_eq!(
            classify(&error_with(
                json!({ "activeTurnNotSteerable": { "turnKind": "review" } })
            )),
            FailureCause::Unknown
        );
        assert_eq!(classify(&Value::Null), FailureCause::Unknown);
    }

    #[test]
    fn malformed_and_ambiguous_errors_cannot_authorize_reassignment() {
        assert_eq!(
            classify(&json!({"codexErrorInfo":"usageLimitExceeded"})),
            FailureCause::Unknown
        );
        for info in [
            json!({"httpConnectionFailed":{},"futureCause":{}}),
            json!({"responseTooManyFailedAttempts":{},"responseStreamDisconnected":{}}),
        ] {
            assert_eq!(classify(&error_with(info)), FailureCause::Unknown);
        }
    }

    #[test]
    fn only_terminal_failures_are_correlated_and_the_original_error_is_retained() {
        let error = json!({"message":"request failed","codexErrorInfo":"usageLimitExceeded","additionalDetails":"original provider detail"});
        let mut event = json!({"method":"turn/completed","params":{"threadId":"owned-thread","turn":{"id":"owned-turn","status":"failed","error":error}}});
        let record = TurnFailure::from_event(&event).unwrap();
        assert_eq!(record.thread_id, "owned-thread");
        assert_eq!(record.turn_id, "owned-turn");
        assert_eq!(record.error, error);
        assert_eq!(record.cause, FailureCause::Quota);
        event["params"]["turn"]["status"] = json!("interrupted");
        assert!(TurnFailure::from_event(&event).is_none());
        event["method"] = json!("error");
        event["params"]["willRetry"] = json!(true);
        assert!(TurnFailure::from_event(&event).is_none());
    }
}
