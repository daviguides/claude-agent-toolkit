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

use crate::mcp_server::SdkMcpServer;

/// External MCP server configurations, keyed by server name.
pub type McpServers = HashMap<String, McpServerConfig>;

/// The `mcp_servers` option: either inline server configs, or a path
/// to an MCP config JSON file the CLI reads itself.
///
/// No `PartialEq`: an inline `Sdk` server config holds live `Arc<dyn
/// Fn>` tool handlers with no sensible equality (see
/// [`McpServerConfig`]'s doc comment).
#[derive(Debug, Clone)]
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
///
/// Does not derive `Serialize`/`PartialEq`: the `Sdk` variant holds
/// live `Arc<dyn Fn>` tool handlers, which implement neither. Wire
/// serialization goes through [`to_cli_config_json`] instead (see
/// `DEVIATIONS.md`).
#[derive(Debug, Clone)]
pub enum McpServerConfig {
    /// Subprocess (stdio) MCP server.
    Stdio {
        /// Executable to launch.
        command: String,
        /// Arguments.
        args: Vec<String>,
        /// Environment variables.
        env: HashMap<String, String>,
    },
    /// Server-sent-events MCP server.
    Sse {
        /// Endpoint URL.
        url: String,
        /// Extra headers.
        headers: HashMap<String, String>,
    },
    /// Streamable HTTP MCP server.
    Http {
        /// Endpoint URL.
        url: String,
        /// Extra headers.
        headers: HashMap<String, String>,
    },
    /// In-process ("sdk") MCP server: tools run inside this process,
    /// never spawned as an external subprocess. Serializes to the
    /// wire as the stub `{"type":"sdk","name":...}` — the handler
    /// table stays SDK-side.
    Sdk(SdkMcpServer),
}

/// Wire representation for `--mcp-config`. The sole serialization path
/// for [`McpServerConfig`] (which cannot derive `Serialize` — see its
/// doc comment).
#[must_use]
pub fn to_cli_config_json(servers: &McpServers) -> Value {
    let entries: serde_json::Map<String, Value> = servers
        .iter()
        .map(|(name, config)| (name.clone(), config.to_config_value()))
        .collect();
    serde_json::json!({ "mcpServers": entries })
}

impl McpServerConfig {
    fn to_config_value(&self) -> Value {
        match self {
            Self::Stdio { command, args, env } => {
                let mut value = serde_json::json!({"type": "stdio", "command": command});
                if !args.is_empty() {
                    value["args"] = serde_json::json!(args);
                }
                if !env.is_empty() {
                    value["env"] = serde_json::json!(env);
                }
                value
            }
            Self::Sse { url, headers } => {
                let mut value = serde_json::json!({"type": "sse", "url": url});
                if !headers.is_empty() {
                    value["headers"] = serde_json::json!(headers);
                }
                value
            }
            Self::Http { url, headers } => {
                let mut value = serde_json::json!({"type": "http", "url": url});
                if !headers.is_empty() {
                    value["headers"] = serde_json::json!(headers);
                }
                value
            }
            Self::Sdk(server) => serde_json::json!({"type": "sdk", "name": server.name}),
        }
    }
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
            "sdk" => Err(de::Error::custom(
                "sdk mcp servers cannot be deserialized from JSON; construct them with \
                 create_sdk_mcp_server() instead",
            )),
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
        let json = config.to_config_value();
        let parsed: McpServerConfig = serde_json::from_value(json).expect("deserializes");
        assert!(
            matches!(parsed, McpServerConfig::Http { url, .. } if url == "https://example.com")
        );
    }

    #[test]
    fn sdk_config_serializes_as_stub_without_handlers() {
        let server = crate::mcp_server::create_sdk_mcp_server("calc", "1.0.0", vec![]);
        let config = McpServerConfig::Sdk(server);
        assert_eq!(
            config.to_config_value(),
            serde_json::json!({"type": "sdk", "name": "calc"})
        );
    }

    #[test]
    fn sdk_config_cannot_be_deserialized() {
        let value = serde_json::json!({"type": "sdk", "name": "calc"});
        let result: Result<McpServerConfig, _> = serde_json::from_value(value);
        assert!(result.is_err());
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
        assert!(matches!(
            McpServersOption::default(),
            McpServersOption::Servers(servers) if servers.is_empty()
        ));
    }
}
