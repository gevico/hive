use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::{HiveError, HiveResult};

/// Top-level hive configuration (merged from config.yml + config.local.yml).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HiveConfig {
    #[serde(default)]
    pub user: UserConfig,
    #[serde(default)]
    pub launch: LaunchConfig,
    #[serde(default)]
    pub rfc: RfcConfig,
    #[serde(default)]
    pub audit_level: AuditLevel,
    #[serde(default)]
    pub skills: SkillsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchConfig {
    #[serde(default = "default_launch_tool")]
    pub tool: String,
    #[serde(default)]
    pub custom_command: Option<String>,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            tool: default_launch_tool(),
            custom_command: None,
        }
    }
}

fn default_launch_tool() -> String {
    "claude".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfcConfig {
    #[serde(default = "default_platform")]
    pub platform: String,
}

impl Default for RfcConfig {
    fn default() -> Self {
        Self {
            platform: default_platform(),
        }
    }
}

fn default_platform() -> String {
    "none".into()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditLevel {
    Minimal,
    #[default]
    Standard,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsConfig {
    #[serde(default)]
    pub default: Vec<String>,
}

/// Source annotation for config display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Global,
    Local,
    Default,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Global => write!(f, "global"),
            Self::Local => write!(f, "local override"),
            Self::Default => write!(f, "default"),
        }
    }
}

/// Deep-merge two YAML values. `overlay` fields override `base` at each level.
pub fn deep_merge(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Mapping(b), Value::Mapping(o)) => {
            let mut merged = b.clone();
            for (key, oval) in o {
                let new_val = if let Some(bval) = b.get(key) {
                    deep_merge(bval, oval)
                } else {
                    oval.clone()
                };
                merged.insert(key.clone(), new_val);
            }
            Value::Mapping(merged)
        }
        (_, overlay) => overlay.clone(),
    }
}

/// Load and merge config from .hive/config.yml and .hive/config.local.yml.
pub fn load_config(hive_dir: &Path) -> HiveResult<HiveConfig> {
    let global_path = hive_dir.join("config.yml");
    let local_path = hive_dir.join("config.local.yml");

    let global_val = load_yaml_file(&global_path)?;
    let local_val = if local_path.exists() {
        load_yaml_file(&local_path)?
    } else {
        Value::Mapping(serde_yaml::Mapping::new())
    };

    let merged = deep_merge(&global_val, &local_val);
    let config: HiveConfig = serde_yaml::from_value(merged)
        .map_err(|e| HiveError::Config(format!("failed to parse merged config: {e}")))?;

    Ok(config)
}

fn load_yaml_file(path: &Path) -> HiveResult<Value> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| HiveError::Config(format!("failed to read {}: {e}", path.display())))?;
    let value: Value = serde_yaml::from_str(&content)
        .map_err(|e| HiveError::Config(format!("malformed YAML in {}: {e}", path.display())))?;
    Ok(value)
}

/// Generate annotated config display comparing global vs local values.
pub fn show_config(hive_dir: &Path) -> HiveResult<Vec<(String, String, ConfigSource)>> {
    let global_path = hive_dir.join("config.yml");
    let local_path = hive_dir.join("config.local.yml");

    let global_val = load_yaml_file(&global_path)?;
    let local_val = if local_path.exists() {
        Some(load_yaml_file(&local_path)?)
    } else {
        None
    };

    let mut entries = Vec::new();
    collect_annotated("", &global_val, local_val.as_ref(), &mut entries);
    Ok(entries)
}

fn collect_annotated(
    prefix: &str,
    global: &Value,
    local: Option<&Value>,
    out: &mut Vec<(String, String, ConfigSource)>,
) {
    if let Value::Mapping(gmap) = global {
        // Collect all keys from both global and local
        let mut all_keys: Vec<&Value> = gmap.keys().collect();
        if let Some(Value::Mapping(lmap)) = local {
            for key in lmap.keys() {
                if !all_keys.contains(&key) {
                    all_keys.push(key);
                }
            }
        }

        for key in all_keys {
            let key_str = key.as_str().unwrap_or("?");
            let full_key = if prefix.is_empty() {
                key_str.to_string()
            } else {
                format!("{prefix}.{key_str}")
            };

            let gval = gmap.get(key);
            let lval = local.and_then(|l| l.as_mapping()).and_then(|m| m.get(key));

            match (gval, lval) {
                (Some(g), Some(l)) => {
                    if g.is_mapping() || l.is_mapping() {
                        collect_annotated(&full_key, g, Some(l), out);
                    } else {
                        out.push((full_key, format_value(l), ConfigSource::Local));
                    }
                }
                (Some(g), None) => {
                    if g.is_mapping() {
                        collect_annotated(&full_key, g, None, out);
                    } else {
                        out.push((full_key, format_value(g), ConfigSource::Global));
                    }
                }
                (None, Some(l)) => {
                    if l.is_mapping() {
                        let empty = Value::Mapping(serde_yaml::Mapping::new());
                        collect_annotated(&full_key, &empty, Some(l), out);
                    } else {
                        out.push((full_key, format_value(l), ConfigSource::Local));
                    }
                }
                (None, None) => {}
            }
        }
    }
}

fn format_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Sequence(seq) => {
            let items: Vec<String> = seq.iter().map(format_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Null => "null".into(),
        other => format!("{other:?}"),
    }
}

/// Get the effective user name from config or git config.
pub fn resolve_user_name(config: &HiveConfig) -> HiveResult<String> {
    if let Some(ref name) = config.user.name {
        return Ok(name.clone());
    }
    // Fall back to git config user.email (part before @)
    let output = std::process::Command::new("git")
        .args(["config", "user.email"])
        .output()
        .map_err(|e| HiveError::Git(format!("failed to run git config: {e}")))?;

    if !output.status.success() {
        return Err(HiveError::Config(
            "git user.email not configured and no user.name in config".into(),
        ));
    }

    let email = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let name = email
        .split('@')
        .next()
        .ok_or_else(|| HiveError::Config("invalid email format".into()))?
        .to_string();

    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_merge_scalars() {
        let base: Value = serde_yaml::from_str("key: base_val").unwrap();
        let overlay: Value = serde_yaml::from_str("key: overlay_val").unwrap();
        let merged = deep_merge(&base, &overlay);
        let m = merged.as_mapping().unwrap();
        assert_eq!(
            m.get(Value::String("key".into()))
                .unwrap()
                .as_str()
                .unwrap(),
            "overlay_val"
        );
    }

    #[test]
    fn deep_merge_nested() {
        let base: Value = serde_yaml::from_str("user:\n  name: alice\n  email: a@b.c").unwrap();
        let overlay: Value = serde_yaml::from_str("user:\n  name: bob").unwrap();
        let merged = deep_merge(&base, &overlay);
        let user = merged
            .as_mapping()
            .unwrap()
            .get(Value::String("user".into()))
            .unwrap()
            .as_mapping()
            .unwrap();
        assert_eq!(
            user.get(Value::String("name".into()))
                .unwrap()
                .as_str()
                .unwrap(),
            "bob"
        );
        // email preserved from base
        assert_eq!(
            user.get(Value::String("email".into()))
                .unwrap()
                .as_str()
                .unwrap(),
            "a@b.c"
        );
    }

    #[test]
    fn deep_merge_adds_new_keys() {
        let base: Value = serde_yaml::from_str("a: 1").unwrap();
        let overlay: Value = serde_yaml::from_str("b: 2").unwrap();
        let merged = deep_merge(&base, &overlay);
        let m = merged.as_mapping().unwrap();
        assert!(m.get(Value::String("a".into())).is_some());
        assert!(m.get(Value::String("b".into())).is_some());
    }

    #[test]
    fn parse_config_struct() {
        let yaml = r#"
user:
  name: testuser
launch:
  tool: codex
rfc:
  platform: github
audit_level: full
skills:
  default:
    - humanize
"#;
        let config: HiveConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.user.name.as_deref(), Some("testuser"));
        assert_eq!(config.launch.tool, "codex");
        assert_eq!(config.rfc.platform, "github");
        assert_eq!(config.audit_level, AuditLevel::Full);
        assert_eq!(config.skills.default, vec!["humanize"]);
    }

    #[test]
    fn config_defaults() {
        let config: HiveConfig = serde_yaml::from_str("{}").unwrap();
        assert_eq!(config.launch.tool, "claude");
        assert_eq!(config.rfc.platform, "none");
        assert_eq!(config.audit_level, AuditLevel::Standard);
    }

    #[test]
    fn show_config_annotations() {
        let base: Value = serde_yaml::from_str("a: 1\nb: 2").unwrap();
        let overlay: Value = serde_yaml::from_str("b: 3").unwrap();
        let mut entries = Vec::new();
        collect_annotated("", &base, Some(&overlay), &mut entries);
        // a should be global, b should be local
        assert!(
            entries
                .iter()
                .any(|(k, _, s)| k == "a" && *s == ConfigSource::Global)
        );
        assert!(
            entries
                .iter()
                .any(|(k, _, s)| k == "b" && *s == ConfigSource::Local)
        );
    }

    fn collect_annotated_wrapper() -> Vec<(String, String, ConfigSource)> {
        let global: Value = serde_yaml::from_str("user:\n  name: alice").unwrap();
        let local: Value = serde_yaml::from_str("user:\n  name: bob").unwrap();
        let mut entries = Vec::new();
        collect_annotated("", &global, Some(&local), &mut entries);
        entries
    }

    #[test]
    fn nested_annotation_shows_local_override() {
        let entries = collect_annotated_wrapper();
        assert!(
            entries
                .iter()
                .any(|(k, v, s)| k == "user.name" && v == "bob" && *s == ConfigSource::Local)
        );
    }
}
