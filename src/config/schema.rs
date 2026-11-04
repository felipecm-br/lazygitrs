//! JSON Schema for `config.yml`, derived from `UserConfig::default()`.
//!
//! Walking the serialized defaults (the same source `find_unknown_keys`
//! uses) keeps the schema in sync with the code without a derive dependency
//! on every config struct. It provides key completion, value types and
//! defaults in editors (`# yaml-language-server: $schema=...`).
//!
//! The schema is deliberately permissive about extra keys: the config file is
//! often shared with lazygit, whose keys lazygitrs does not know. Typos are
//! reported at startup instead (`UserConfig::find_unknown_keys`).

use serde_json::{Map, Value, json};

use super::user_config::{CustomCommand, CustomCommandPrompt, UserConfig};

/// The full JSON Schema document.
pub fn config_json_schema() -> Value {
    let defaults = serde_json::to_value(UserConfig::default()).unwrap_or(Value::Null);
    let mut root = infer(&defaults, "");
    if let Value::Object(map) = &mut root {
        map.insert(
            "$schema".into(),
            json!("https://json-schema.org/draft/2020-12/schema"),
        );
        map.insert("title".into(), json!("lazygitrs config.yml"));
        map.insert(
            "description".into(),
            json!(
                "lazygitrs configuration (lazygit-compatible). Generated from the built-in defaults; \
                 regenerate with `lazygitrs --print-config-schema`."
            ),
        );
    }
    root
}

/// Schema for a value at dotted `path`, with its default.
fn infer(value: &Value, path: &str) -> Value {
    if let Some(special) = special_case(value, path) {
        return special;
    }
    let mut schema = match value {
        Value::Object(map) => {
            let props: Map<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), infer(v, &join(path, k))))
                .collect();
            return json!({ "type": "object", "properties": props });
        }
        Value::Array(items) => {
            let item = items.first().map(|v| infer(v, path)).unwrap_or(json!({}));
            json!({ "type": "array", "items": item })
        }
        Value::String(_) => json!({ "type": "string" }),
        Value::Bool(_) => json!({ "type": "boolean" }),
        Value::Number(n) if n.is_u64() || n.is_i64() => json!({ "type": "integer" }),
        Value::Number(_) => json!({ "type": "number" }),
        // Optional values whose default is unset: accept anything.
        Value::Null => return json!({}),
    };
    if let Value::Object(map) = &mut schema {
        map.insert("default".into(), value.clone());
    }
    schema
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

/// Keys whose accepted shape differs from their default's shape. These
/// mirror the exceptions in `find_unknown_keys`.
fn special_case(value: &Value, path: &str) -> Option<Value> {
    match path {
        // `theme: omarchy` or a lazygit color map (optionally with `preset`).
        "gui.theme" => {
            let mut map_schema = infer_plain_object(value, path);
            if let Some(Value::Object(props)) = map_schema.get_mut("properties") {
                props.insert(
                    "preset".into(),
                    json!({ "type": "string", "description": "Theme preset name" }),
                );
            }
            Some(json!({
                "description": "Theme preset name (e.g. `omarchy`, `cockpit`) or a lazygit color map",
                "oneOf": [
                    { "type": "string", "description": "Theme preset name" },
                    map_schema,
                ],
            }))
        }
        // `border: rounded` or a per-panel map.
        "gui.border" => Some(json!({
            "description": "Border style for all panels, or a map of panel name to style",
            "oneOf": [
                { "type": "string" },
                { "type": "object", "additionalProperties": { "type": "string" } },
            ],
            "default": value,
        })),
        // Empty by default: describe the item shape from a filled sample.
        "customCommands" => {
            let sample = serde_json::to_value(CustomCommand {
                key: String::new(),
                context: String::new(),
                command: String::new(),
                description: String::new(),
                stream: false,
                show_output: false,
                prompts: vec![CustomCommandPrompt {
                    prompt_type: Some(String::new()),
                    title: Some(String::new()),
                    key: Some(String::new()),
                    command: Some(String::new()),
                    filter: Some(String::new()),
                }],
            })
            .ok()?;
            let mut item = infer_plain_object(&sample, path);
            if let Value::Object(map) = &mut item {
                map.insert(
                    "required".into(),
                    json!(["key", "context", "command", "description"]),
                );
            }
            Some(json!({ "type": "array", "items": item, "default": [] }))
        }
        _ => None,
    }
}

/// Object schema without re-entering `special_case` for `path` itself.
fn infer_plain_object(value: &Value, path: &str) -> Value {
    let Value::Object(map) = value else {
        return json!({ "type": "object" });
    };
    let props: Map<String, Value> = map
        .iter()
        .map(|(k, v)| (k.clone(), infer(v, &join(path, k))))
        .collect();
    json!({ "type": "object", "properties": props })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at<'a>(schema: &'a Value, path: &[&str]) -> &'a Value {
        path.iter().fold(schema, |s, key| &s["properties"][*key])
    }

    #[test]
    fn covers_every_default_key_with_its_type_and_default() {
        let schema = config_json_schema();
        assert_eq!(schema["type"], "object");
        let interval = at(&schema, &["refresher", "refreshInterval"]);
        assert_eq!(interval["type"], "integer");
        assert_eq!(interval["default"], 10);
        assert_eq!(at(&schema, &["git", "autoFetch"])["type"], "boolean");
        let quit = at(&schema, &["keybinding", "universal", "quit"]);
        assert_eq!(quit["type"], "string");
        assert_eq!(quit["default"], "q");
    }

    #[test]
    fn theme_accepts_a_preset_name_or_a_color_map() {
        let schema = config_json_schema();
        let theme = at(&schema, &["gui", "theme"]);
        let variants = theme["oneOf"].as_array().expect("oneOf");
        assert_eq!(variants[0]["type"], "string");
        assert!(variants[1]["properties"]["preset"].is_object());
        assert!(variants[1]["properties"]["activeBorderColor"].is_object());
    }

    #[test]
    fn custom_commands_describe_their_items() {
        let schema = config_json_schema();
        let items = &at(&schema, &["customCommands"])["items"];
        assert_eq!(items["properties"]["context"]["type"], "string");
        assert_eq!(
            items["properties"]["prompts"]["items"]["properties"]["title"]["type"],
            "string"
        );
        assert!(
            items["required"]
                .as_array()
                .unwrap()
                .contains(&json!("command"))
        );
    }

    #[test]
    fn known_keys_present_and_typos_absent() {
        // Keys the startup warning accepts must exist in the schema, and a
        // typo must not.
        let schema = config_json_schema();
        assert!(at(&schema, &["gui", "borderGradient"]).is_object());
        assert!(at(&schema, &["gui", "borderGradiant"]).is_null());
    }

    #[test]
    fn committed_schema_file_is_up_to_date() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/_docs/config.schema.json");
        let committed = std::fs::read_to_string(path).expect("read _docs/config.schema.json");
        let generated = serde_json::to_string_pretty(&config_json_schema()).unwrap();
        assert_eq!(
            committed.trim_end(),
            generated.trim_end(),
            "_docs/config.schema.json is stale: run `just gen-schema`"
        );
    }
}
