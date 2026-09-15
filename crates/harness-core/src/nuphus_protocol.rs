//! Model-free Nuphus protocol bounds: screenshot conversion, snapshot
//! references and the six-field schema adaptation. No browser, desktop or
//! package process starts here.
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
pub const MAX_SCREENSHOT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_DIAGNOSTIC: usize = 240;
pub const SCHEMA_TOOLS: [&str; 3] = ["browser_click", "browser_type", "browser_drag_files"];
pub const SCREENSHOT_TOOLS: [&str; 2] = ["desktop_screenshot", "desktop_window_screenshot"];
pub const AUDITED_ORIGINALS: [(&str, &str); 1] = [(
    "0.2.2",
    "9a07112f17a964d9c0b1a54653af95559d7de33cce1cb3dffd60dfc4c85ccfb0",
)];

#[derive(Debug)]
pub struct ScreenshotRejected {
    pub message: String,
}

impl ScreenshotRejected {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: bounded_detail(&message.into()),
        }
    }
}

impl std::fmt::Display for ScreenshotRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ScreenshotRejected {}

pub fn bounded_detail(detail: &str) -> String {
    let text = if detail.is_empty() {
        "unknown screenshot conversion failure"
    } else {
        detail
    };
    text.chars().take(MAX_DIAGNOSTIC).collect()
}

fn decode_base64(payload: &str) -> Result<Vec<u8>, ScreenshotRejected> {
    let mut compact: String = payload.split_whitespace().collect();
    if let Some(rest) = compact.strip_prefix("data:") {
        let (header, data) = rest.split_once(',').ok_or_else(|| {
            ScreenshotRejected::new("Screenshot data URL is not a bounded base64 image")
        })?;
        if !header.to_ascii_lowercase().contains(";base64") || data.is_empty() {
            return Err(ScreenshotRejected::new(
                "Screenshot data URL is not a bounded base64 image",
            ));
        }
        compact = data.to_owned();
    }
    let padding = (4 - compact.len() % 4) % 4;
    compact.extend(std::iter::repeat_n('=', padding));
    base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        compact.as_bytes(),
    )
    .map_err(|_| ScreenshotRejected::new("Screenshot payload is not valid base64"))
}

fn image_bytes(value: &Value) -> Result<Vec<u8>, ScreenshotRejected> {
    match value {
        Value::String(text) => {
            let stripped = text.trim();
            if (stripped.starts_with('{') || stripped.starts_with('['))
                && let Ok(parsed) = serde_json::from_str::<Value>(stripped)
            {
                return image_bytes(&parsed);
            }
            decode_base64(stripped)
        }
        Value::Object(map) => {
            if let Some(data) = map.get("data") {
                return image_bytes(data);
            }
            if let Some(Value::Array(content)) = map.get("content")
                && let Some(first) = content.first()
            {
                return image_bytes(first);
            }
            if let Some(text) = map.get("text") {
                return image_bytes(text);
            }
            Err(ScreenshotRejected::new(
                "Screenshot object has no image data",
            ))
        }
        Value::Array(items) => items
            .first()
            .ok_or_else(|| ScreenshotRejected::new("Screenshot payload is not an image"))
            .and_then(image_bytes),
        _ => Err(ScreenshotRejected::new(
            "Screenshot payload is not an image",
        )),
    }
}

fn png_mime(raw: &[u8]) -> Result<&'static str, ScreenshotRejected> {
    if raw.starts_with(PNG_SIGNATURE) {
        Ok("image/png")
    } else {
        Err(ScreenshotRejected::new(
            "Screenshot payload is not a PNG image",
        ))
    }
}

fn text_contains_image(text: &str) -> bool {
    let compact: String = text.split_whitespace().collect();
    compact.contains("iVBORw0KGgo") || compact.starts_with("data:image/")
}

fn result_text(result: &Value) -> String {
    result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "text")
        .filter_map(|item| item["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert a screenshot tool result. Path arguments stay path-only; otherwise
/// image bytes become one native image content block and structuredContent is
/// cleared. Original oversized or invalid payloads never enter the result.
pub fn bound_screenshot_result(
    result: Value,
    arguments: &Value,
) -> Result<Value, ScreenshotRejected> {
    let path = arguments["path"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(path) = path {
        if result["isError"] == true {
            return Ok(result);
        }
        let text = result_text(&result);
        if text_contains_image(&text) {
            return Err(ScreenshotRejected::new(
                "Path screenshot result still contains image bytes",
            ));
        }
        let text = if text.contains(path) {
            text
        } else if text.is_empty() {
            path.to_owned()
        } else {
            format!("{text}\n{path}")
        };
        let mut bounded = result;
        bounded["content"] = json!([{"type":"text","text":text}]);
        return Ok(bounded);
    }
    let raw = if !result["structuredContent"].is_null() {
        match image_bytes(&result["structuredContent"]) {
            Ok(raw) => raw,
            Err(_) => {
                let text = result_text(&result);
                if text.is_empty() {
                    return Err(ScreenshotRejected::new(
                        "Screenshot result has no image payload",
                    ));
                }
                image_bytes(&Value::String(text))?
            }
        }
    } else {
        let text = result_text(&result);
        if text.is_empty() {
            return Err(ScreenshotRejected::new(
                "Screenshot result has no image payload",
            ));
        }
        image_bytes(&Value::String(text))?
    };
    if raw.len() > MAX_SCREENSHOT_BYTES {
        return Err(ScreenshotRejected::new(format!(
            "Screenshot exceeds {MAX_SCREENSHOT_BYTES} bytes"
        )));
    }
    let mime = png_mime(&raw)?;
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &raw);
    Ok(json!({
        "content": [{"type":"image","data":encoded,"mimeType":mime}],
        "structuredContent": Value::Null,
        "isError": false
    }))
}

/// Bind upstream @N snapshot handles to this proxy and exact snapshot.
#[derive(Default)]
pub struct BrowserReferences {
    references: BTreeMap<String, String>,
}

impl BrowserReferences {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn expire(&mut self) {
        self.references.clear();
    }

    pub fn arguments(&self, mut arguments: Value) -> Result<Value, String> {
        let Some(reference) = arguments
            .get("ref")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return Ok(arguments);
        };
        let Some(original) = self.references.get(&reference) else {
            return Err(
                "Browser reference expired or belongs to another session; take a fresh browser_snapshot"
                    .into(),
            );
        };
        arguments["ref"] = json!(original);
        Ok(arguments)
    }

    pub fn snapshot(&mut self, mut result: Value) -> Result<Value, String> {
        if result["isError"] == true {
            return Ok(result);
        }
        self.expire();
        let prefix = crate::broker_endpoint::random_key()
            .map(|key| key[..12].to_owned())
            .unwrap_or_else(|_| format!("{:012x}", std::process::id() as u64));
        let mut count = 0usize;
        let transformed = transform_value(&result, &prefix, &mut self.references, &mut count)?;
        if let Some(content) = transformed.get("content") {
            result["content"] = content.clone();
        }
        if transformed.get("structuredContent").is_some() {
            result["structuredContent"] = transformed["structuredContent"].clone();
        }
        Ok(result)
    }
}

fn transform_value(
    value: &Value,
    prefix: &str,
    references: &mut BTreeMap<String, String>,
    count: &mut usize,
) -> Result<Value, String> {
    match value {
        Value::String(text) => {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                let transformed = transform_value(&parsed, prefix, references, count)?;
                Ok(Value::String(
                    serde_json::to_string(&transformed).unwrap_or_else(|_| text.clone()),
                ))
            } else {
                Ok(Value::String(replace_refs(
                    text, prefix, references, count,
                )?))
            }
        }
        Value::Array(items) => Ok(Value::Array(
            items
                .iter()
                .map(|item| transform_value(item, prefix, references, count))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, item) in map {
                out.insert(
                    key.clone(),
                    transform_value(item, prefix, references, count)?,
                );
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

fn replace_refs(
    text: &str,
    prefix: &str,
    references: &mut BTreeMap<String, String>,
    count: &mut usize,
) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'@'
            && (index == 0 || bytes[index - 1].is_ascii_whitespace())
            && index + 1 < bytes.len()
            && bytes[index + 1].is_ascii_digit()
        {
            let mut end = index + 1;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            let rest = &text[end..];
            if rest.starts_with(" [") {
                if *count >= 10_000 {
                    return Err(
                        "Browser snapshot exceeds 10000 reference bound; request a narrower snapshot"
                            .into(),
                    );
                }
                let number = &text[index + 1..end];
                let reference = format!("@{prefix}:{number}");
                references.insert(reference.clone(), format!("@{number}"));
                *count += 1;
                out.push_str(&reference);
                index = end;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    Ok(out)
}

/// Adapt the three dual-selector tools so each anyOf branch is a typed object.
pub fn adapt_schema(mut tool: Value) -> Result<Value, String> {
    let Some(name) = tool["name"].as_str().map(str::to_owned) else {
        return Ok(tool);
    };
    if !SCHEMA_TOOLS.contains(&name.as_str()) {
        return Ok(tool);
    }
    let schema = tool
        .get_mut("inputSchema")
        .ok_or_else(|| format!("Unreviewed Nuphus schema: {name}"))?;
    if schema["type"] != "object" {
        return Err(format!("Unreviewed Nuphus schema: {name}"));
    }
    let Some(branches) = schema["anyOf"].as_array_mut() else {
        return Err(format!("Unreviewed Nuphus schema: {name}"));
    };
    if branches.len() != 2 {
        return Err(format!("Unreviewed Nuphus schema: {name}"));
    }
    for (branch, key) in branches.iter_mut().zip(["selector", "ref"]) {
        let required = branch["required"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(Value::as_str);
        if required != Some(key) {
            return Err(format!("Unreviewed Nuphus alternative schema: {name}"));
        }
        if let Some(object) = branch.as_object() {
            for field in object.keys() {
                if field != "required" && field != "type" {
                    return Err(format!("Unreviewed Nuphus alternative schema: {name}"));
                }
            }
        }
        if branch.get("type").is_some_and(|value| value != "object") {
            return Err(format!("Unreviewed Nuphus alternative schema: {name}"));
        }
        branch["type"] = json!("object");
    }
    Ok(tool)
}

pub fn audited_digest(version: &str) -> Option<&'static str> {
    AUDITED_ORIGINALS
        .iter()
        .find(|(known, _)| *known == version)
        .map(|(_, digest)| *digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes() -> Vec<u8> {
        decode_base64(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGNgYGAAAAAEAAH2FzhVAAAAAElFTkSuQmCC",
        )
        .unwrap()
    }

    #[test]
    fn nested_text_png_becomes_image_block() {
        let raw = png_bytes();
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &raw);
        let nested = serde_json::to_string(&json!({"content":[{"type":"text","text": serde_json::to_string(&json!({"data":encoded})).unwrap()}]}))
            .unwrap();
        let result = json!({"content":[{"type":"text","text":nested}]});
        let converted = bound_screenshot_result(result, &json!({})).unwrap();
        assert_eq!(converted["content"][0]["type"], "image");
        assert_eq!(converted["content"][0]["mimeType"], "image/png");
        assert!(
            converted["content"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["type"] != "text")
        );
        assert!(converted["structuredContent"].is_null());
        let data = converted["content"][0]["data"].as_str().unwrap();
        assert_eq!(
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).unwrap(),
            raw
        );
    }

    #[test]
    fn path_result_stays_path_only() {
        let path = r"C:\tmp\owned.png";
        let result = json!({"content":[{"type":"text","text":"saved screenshot"}]});
        let converted = bound_screenshot_result(result, &json!({"path": path})).unwrap();
        assert_eq!(converted["content"][0]["type"], "text");
        assert!(
            converted["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains(path)
        );
        assert!(
            !converted["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("iVBORw0KGgo")
        );
    }

    #[test]
    fn path_result_with_image_text_is_rejected() {
        let encoded =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png_bytes());
        let result = json!({"content":[{"type":"text","text":encoded}]});
        let error =
            bound_screenshot_result(result, &json!({"path": r"C:\tmp\owned.png"})).unwrap_err();
        assert!(error.message.contains("image bytes"));
    }

    #[test]
    fn invalid_payload_is_bounded_rejection() {
        let result = json!({"content":[{"type":"text","text":"not-an-image"}]});
        let error = bound_screenshot_result(result, &json!({})).unwrap_err();
        assert!(error.message.len() <= MAX_DIAGNOSTIC);
        assert!(!error.message.contains("iVBORw0KGgo"));
    }

    #[test]
    fn oversized_payload_is_rejected_without_image_text() {
        let huge = [png_bytes(), vec![0; MAX_SCREENSHOT_BYTES + 1]].concat();
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &huge);
        let result = json!({"content":[{"type":"text","text":encoded}]});
        let error = bound_screenshot_result(result, &json!({})).unwrap_err();
        assert!(error.message.contains("exceeds"));
        assert!(error.message.len() <= MAX_DIAGNOSTIC);
        assert!(!error.message.contains("iVBORw0KGgo"));
    }

    #[test]
    fn browser_snapshot_is_not_screenshot_conversion() {
        let payload = serde_json::to_string(&json!({"snapshot": "@1 [button] \"Apply\""})).unwrap();
        let result = json!({"content":[{"type":"text","text":payload}]});
        let mut refs = BrowserReferences::new();
        let transformed = refs.snapshot(result).unwrap();
        assert_eq!(transformed["content"][0]["type"], "text");
        assert!(
            transformed["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("[button]")
        );
    }

    #[test]
    fn ref_is_bound_to_proxy_snapshot_and_survives_no_restart_collision() {
        let payload = serde_json::to_string(
            &json!({"snapshot": "@1 [button] \"Apply\"\n@2 [textbox] \"Value\""}),
        )
        .unwrap();
        let result = json!({"content":[{"type":"text","text":payload}]});
        let mut first = BrowserReferences::new();
        let second = BrowserReferences::new();
        let transformed = first.snapshot(result.clone()).unwrap();
        let reference = first.references.keys().next().unwrap().clone();
        assert!(
            transformed["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains(&reference)
        );
        assert_eq!(
            first.arguments(json!({"ref": reference.clone()})).unwrap()["ref"],
            "@1"
        );
        assert!(first.arguments(json!({"ref": "@1"})).is_err());
        assert!(second.arguments(json!({"ref": reference.clone()})).is_err());
        first.expire();
        first.snapshot(result).unwrap();
        assert!(first.arguments(json!({"ref": reference})).is_err());
        assert_eq!(first.references.len(), 2);
    }

    #[test]
    fn adapt_schema_types_anyof_branches() {
        let tool = json!({
            "name": "browser_click",
            "inputSchema": {
                "type": "object",
                "anyOf": [{"required":["selector"]}, {"required":["ref"]}]
            }
        });
        let adapted = adapt_schema(tool).unwrap();
        assert_eq!(adapted["inputSchema"]["anyOf"][0]["type"], "object");
        assert_eq!(adapted["inputSchema"]["anyOf"][1]["type"], "object");
    }

    #[test]
    fn adapt_schema_rejects_unreviewed_shape() {
        let tool = json!({
            "name": "browser_click",
            "inputSchema": {"type": "object", "anyOf": [{"required":["selector"], "extra": true}, {"required":["ref"]}]}
        });
        assert!(adapt_schema(tool).is_err());
    }
}
