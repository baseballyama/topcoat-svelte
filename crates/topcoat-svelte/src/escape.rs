//! Serializing island props into XSS-safe JSON for a `<script>` element.

use serde::Serialize;

/// Serializes `value` to JSON, escaping the characters that could let the text
/// break out of the surrounding `<script type="application/json">` element or be
/// misread by the HTML parser.
///
/// `<`, `>` and `&` only ever appear inside JSON string values, where the
/// `\uXXXX` form is an exact equivalent, so escaping them changes what the HTML
/// parser sees without changing the parsed JSON. The line/paragraph separators
/// are escaped as a courtesy; they are valid in JSON but historically hazardous
/// in a script context.
pub(crate) fn to_script_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    Ok(escape(&serde_json::to_string(value)?))
}

fn escape(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for ch in json.chars() {
        match ch {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_script_breakout() {
        let json = to_script_json(&serde_json::json!({
            "html": "</script><script>alert(1)</script>"
        }))
        .unwrap();
        assert!(!json.contains("</script>"));
        assert!(!json.contains('<'));
        assert!(!json.contains('>'));
        assert!(json.contains("\\u003c/script\\u003e"));
    }

    #[test]
    fn escapes_ampersand_and_separators() {
        let json = to_script_json(&serde_json::json!({ "a": "x&y\u{2028}\u{2029}" })).unwrap();
        assert!(!json.contains('&'));
        assert!(json.contains("\\u0026"));
        assert!(json.contains("\\u2028"));
        assert!(json.contains("\\u2029"));
    }

    #[test]
    fn round_trips_as_json() {
        let value = serde_json::json!({ "count": 3, "label": "a<b>c&d" });
        let escaped = to_script_json(&value).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&escaped).unwrap();
        assert_eq!(parsed, value);
    }
}
