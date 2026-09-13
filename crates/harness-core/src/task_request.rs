//! Correlation evidence carried by native Responses requests (CLI 0.154.0).
//! Metadata is not authorization: callers must match an owned native thread,
//! attempt and live view before using it to admit work. Cache keys identify a
//! shared cache context and must never stand in for a conversation identity.
use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub struct RequestIdentity {
    pub thread: String,
    pub session: String,
    pub turn: String,
    pub kind: String,
}

impl RequestIdentity {
    pub fn from_request(request: &Value) -> Result<Self, &'static str> {
        let metadata = &request["client_metadata"];
        let serialized = metadata["x-codex-turn-metadata"]
            .as_str()
            .filter(|text| text.len() <= 16 * 1024)
            .ok_or("native request correlation metadata is missing or exceeds its bound")?;
        let nested: Value = serde_json::from_str(serialized)
            .map_err(|_| "native request correlation metadata is invalid")?;
        let field = |key: &str| -> Result<String, &'static str> {
            let value = metadata[key]
                .as_str()
                .filter(|value| !value.is_empty() && value.len() <= 256)
                .ok_or("native request identity is missing or invalid")?;
            if nested[key].as_str() != Some(value) {
                return Err("native request identity fields disagree");
            }
            Ok(value.into())
        };
        Ok(Self {
            thread: field("thread_id")?,
            session: field("session_id")?,
            turn: field("turn_id")?,
            kind: nested["request_kind"]
                .as_str()
                .filter(|value| !value.is_empty() && value.len() <= 256)
                .ok_or("native request kind is missing or invalid")?
                .into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(thread: &str) -> Value {
        let fields = json!({"thread_id":thread,"session_id":"shared-session","turn_id":"attempt","request_kind":"turn"});
        json!({"prompt_cache_key":"shared-session","client_metadata":{
            "thread_id":thread,"session_id":"shared-session","turn_id":"attempt",
            "x-codex-turn-metadata":fields.to_string()}})
    }

    #[test]
    fn child_and_parent_with_one_cache_key_remain_distinct() {
        let parent = RequestIdentity::from_request(&request("parent")).unwrap();
        let child = RequestIdentity::from_request(&request("child")).unwrap();
        assert_ne!(parent.thread, child.thread);
        assert_eq!(parent.session, child.session);
        assert_eq!(child.thread, "child");
    }

    #[test]
    fn missing_and_conflicting_metadata_cannot_fall_back_to_cache_identity() {
        assert!(RequestIdentity::from_request(&json!({"prompt_cache_key":"parent"})).is_err());
        for key in ["thread_id", "session_id", "turn_id"] {
            let mut value = request("child");
            value["client_metadata"][key] = json!("other");
            assert!(RequestIdentity::from_request(&value).is_err());
        }
        let mut value = request("child");
        value["client_metadata"]["x-codex-turn-metadata"] = json!("{invalid");
        assert!(RequestIdentity::from_request(&value).is_err());
    }
}
