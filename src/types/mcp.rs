//! MCP server configuration.
//!
//! The in-process ("sdk") server variant is added in Phase 9; its
//! serialized form for `--mcp-config` is `{"type":"sdk","name":...}`
//! with the callback table stripped before serialization.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// External MCP server configurations, keyed by server name.
pub type McpServers = HashMap<String, McpServerConfig>;

/// The `mcp_servers` option: either inline server configs, or a path
/// to an MCP config JSON file the CLI reads itself.
#[derive(Debug, Clone, PartialEq)]
pub enum McpServersOption {
    /// Inline server configurations.
    Servers(McpServers),
    /// Path to an MCP config JSON file (or an inline JSON string),
    /// passed through to the CLI verbatim.
    Path(String),
}

impl Default for McpServersOption {
    fn default() -> Self {
        Self::Servers(McpServers::new())
    }
}

/// One MCP server entry in the configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpServerConfig {
    /// Subprocess (stdio) MCP server.
    Stdio {
        /// Executable to launch.
        command: String,
        /// Arguments.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        /// Environment variables.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
    },
    /// Server-sent-events MCP server.
    Sse {
        /// Endpoint URL.
        url: String,
        /// Extra headers.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        headers: HashMap<String, String>,
    },
    /// Streamable HTTP MCP server.
    Http {
        /// Endpoint URL.
        url: String,
        /// Extra headers.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        headers: HashMap<String, String>,
    },
}

/// Upstream's `type` field is optional on stdio configs (defaults to
/// `"stdio"` when absent), so a derived internally-tagged `Deserialize`
/// (which requires the tag) cannot be used directly.
impl<'de> Deserialize<'de> for McpServerConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let tag = value.get("type").and_then(Value::as_str).unwrap_or("stdio");

        match tag {
            "stdio" => {
                #[derive(Deserialize)]
                struct Stdio {
                    command: String,
                    #[serde(default)]
                    args: Vec<String>,
                    #[serde(default)]
                    env: HashMap<String, String>,
                }
                let stdio: Stdio = serde_json::from_value(value).map_err(de::Error::custom)?;
                Ok(Self::Stdio {
                    command: stdio.command,
                    args: stdio.args,
                    env: stdio.env,
                })
            }
            "sse" => {
                #[derive(Deserialize)]
                struct Sse {
                    url: String,
                    #[serde(default)]
                    headers: HashMap<String, String>,
                }
                let sse: Sse = serde_json::from_value(value).map_err(de::Error::custom)?;
                Ok(Self::Sse {
                    url: sse.url,
                    headers: sse.headers,
                })
            }
            "http" => {
                #[derive(Deserialize)]
                struct Http {
                    url: String,
                    #[serde(default)]
                    headers: HashMap<String, String>,
                }
                let http: Http = serde_json::from_value(value).map_err(de::Error::custom)?;
                Ok(Self::Http {
                    url: http.url,
                    headers: http.headers,
                })
            }
            other => Err(de::Error::custom(format!(
                "unknown mcp server type: {other}"
            ))),
        }
    }
}

/// A path to a local Claude Code plugin directory, or the plugin's own
/// declared type. Mirrors upstream `SdkPluginConfig`; only `"local"` is
/// supported by the CLI today.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginConfig {
    /// Plugin loaded from a local directory.
    Local {
        /// Path to the plugin directory.
        path: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_config_deserializes_without_type_tag() {
        let value = serde_json::json!({"command": "npx", "args": ["-y", "server"]});
        let config: McpServerConfig = serde_json::from_value(value).expect("deserializes");
        assert!(matches!(config, McpServerConfig::Stdio { command, .. } if command == "npx"));
    }

    #[test]
    fn stdio_config_deserializes_with_explicit_type_tag() {
        let value = serde_json::json!({"type": "stdio", "command": "npx"});
        let config: McpServerConfig = serde_json::from_value(value).expect("deserializes");
        assert!(matches!(config, McpServerConfig::Stdio { .. }));
    }

    #[test]
    fn sse_config_requires_type_tag() {
        let value = serde_json::json!({"type": "sse", "url": "https://example.com"});
        let config: McpServerConfig = serde_json::from_value(value).expect("deserializes");
        assert!(matches!(config, McpServerConfig::Sse { url, .. } if url == "https://example.com"));
    }

    #[test]
    fn http_config_round_trips() {
        let config = McpServerConfig::Http {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
        };
        let json = serde_json::to_value(&config).expect("serializes");
        let parsed: McpServerConfig = serde_json::from_value(json).expect("deserializes");
        assert_eq!(parsed, config);
    }

    #[test]
    fn unknown_type_tag_is_rejected() {
        let value = serde_json::json!({"type": "carrier-pigeon", "url": "x"});
        let result: Result<McpServerConfig, _> = serde_json::from_value(value);
        assert!(result.is_err());
    }

    #[test]
    fn plugin_config_local_round_trips() {
        let config = PluginConfig::Local {
            path: PathBuf::from("/plugins/my-plugin"),
        };
        let json = serde_json::to_value(&config).expect("serializes");
        assert_eq!(json["type"], "local");
        let parsed: PluginConfig = serde_json::from_value(json).expect("deserializes");
        assert_eq!(parsed, config);
    }

    #[test]
    fn mcp_servers_option_defaults_to_empty_servers() {
        assert_eq!(
            McpServersOption::default(),
            McpServersOption::Servers(McpServers::new())
        );
    }
}
