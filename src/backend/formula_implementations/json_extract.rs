use serde_json::Value;
use serde_json_path::JsonPath;

pub(super) fn json_extract(input: &str, path: &str) -> Result<String, String> {
    let parsed: Value = serde_json::from_str(input.trim())
        .map_err(|error| format!("Could not parse JSON: {error}"))?;
    let trimmed_path = path.trim();
    if trimmed_path.is_empty() {
        return Err("JSON path cannot be empty".to_string());
    }

    let query = JsonPath::parse(trimmed_path)
        .map_err(|error| format!("Could not parse JSONPath: {error}"))?;
    let matches = query.query(&parsed).all();
    let value = match matches.as_slice() {
        [] => return Ok(String::new()),
        [value] => *value,
        _ => return Err("JSONEXTRACT path must resolve to at most one value".to_string()),
    };

    if value.is_null() {
        return Ok(String::new());
    }

    json_value_to_text(value)
}

fn json_value_to_text(value: &Value) -> Result<String, String> {
    match value {
        Value::Null => Ok(String::new()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(value.clone()),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value)
            .map_err(|error| format!("Could not serialize JSON: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_values() {
        let json =
            r#"{"user":{"name":"Ada","active":true,"details":{"age":37}},"tags":["ai","art"]}"#;

        assert_eq!(json_extract(json, "$.user.name"), Ok("Ada".to_string()));
        assert_eq!(
            json_extract(json, "$.user"),
            Ok(r#"{"active":true,"details":{"age":37},"name":"Ada"}"#.to_string())
        );
    }

    #[test]
    fn extracts_json_arrays_and_returns_empty_text_for_missing_paths() {
        let json = r#"{"items":[[{"name":"first"}],[{"name":"second"}]],"empty":null}"#;

        assert_eq!(
            json_extract(json, "$.items[0][0].name"),
            Ok("first".to_string())
        );
        assert_eq!(
            json_extract(json, "$.items[1]"),
            Ok(r#"[{"name":"second"}]"#.to_string())
        );
        assert_eq!(json_extract(json, "$.missing.path"), Ok(String::new()));
        assert_eq!(json_extract(json, "$.empty"), Ok(String::new()));
    }

    #[test]
    fn rejects_invalid_json_paths() {
        assert_eq!(
            json_extract(r#"{"items":[1]}"#, ""),
            Err("JSON path cannot be empty".to_string())
        );
        assert_eq!(
            json_extract(r#"{"items":[1,2]}"#, "$.items[*]"),
            Err("JSONEXTRACT path must resolve to at most one value".to_string())
        );
    }
}
