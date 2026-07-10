<!-- GENERATED from parity.yaml — do not edit -->
# Upstream <-> Rust Parity

Machine-readable source: [`parity.yaml`](parity.yaml). Regenerate
this file with `scripts/render-parity.sh` after editing it.

Upstream pin: `fdee0adc99f46e65ae9d6d029a6f4fb31bb8cffa`

**Total: 225** — ported: 165, justified_gap: 35, not_ported: 25, partial: 0

## client_method

| Upstream symbol | Kind | Rust equivalent | Status | Tested | Notes / Justification |
|---|---|---|---|---|---|
| `claude_agent_sdk.ContextUsageCategory` | field | `ClaudeClient::get_context_usage -> serde_json::Value` | justified_gap | true | Same raw-Value reasoning as McpServerStatus above. |
| `claude_agent_sdk.ContextUsageResponse` | field | `ClaudeClient::get_context_usage -> serde_json::Value` | justified_gap | true | Same raw-Value reasoning as McpServerStatus above. |
| `ClaudeSDKClient.connect` | method | `ClaudeClient::connect` | ported | true | No initial-prompt parameter -- SubprocessTransport is prompt-agnostic here, so there's no constructor to satisfy (DEVIATIONS.md Phase 7). |
| `ClaudeSDKClient (transport=...)` | method | `ClaudeClient::connect_with_transport` | ported | true | Rust addition mirroring upstream's transport=... constructor kwarg as a dedicated method (DEVIATIONS.md Phase 7). |
| `ClaudeSDKClient.query` | method | `ClaudeClient::send / send_content / send_stream` | ported | true | Split into 3 methods by input shape (string/blocks/stream) instead of one overloaded method; no custom session_id parameter (DEVIATIONS.md Phase 7). |
| `ClaudeSDKClient.receive_response` | method | `ClaudeClient::receive_response` | ported | true |  |
| `ClaudeSDKClient.receive_messages` | method | `ClaudeClient::receive_messages` | ported | true |  |
| `ClaudeSDKClient.interrupt` | method | `ClaudeClient::interrupt` | ported | true |  |
| `ClaudeSDKClient.set_permission_mode` | method | `ClaudeClient::set_permission_mode` | ported | true |  |
| `ClaudeSDKClient.set_model` | method | `ClaudeClient::set_model` | ported | true |  |
| `ClaudeSDKClient.rewind_files` | method | `ClaudeClient::rewind_files` | ported | true | DEVIATIONS.md Phase 7: plan's sketch had 9 methods; upstream has 16, all thin wrappers over Phase 5's Query convenience methods. |
| `ClaudeSDKClient.reconnect_mcp_server` | method | `ClaudeClient::reconnect_mcp_server` | ported | true | reconnect_mcp_server_sends_server_name in tests/client_test.rs. |
| `ClaudeSDKClient.toggle_mcp_server` | method | `ClaudeClient::toggle_mcp_server` | ported | true | toggle_mcp_server_sends_server_name_and_enabled in tests/client_test.rs. |
| `ClaudeSDKClient.stop_task` | method | `ClaudeClient::stop_task` | ported | true | stop_task_sends_task_id in tests/client_test.rs. |
| `ClaudeSDKClient.get_mcp_status` | method | `ClaudeClient::get_mcp_status` | ported | true | get_mcp_status_returns_response_value in tests/client_test.rs. |
| `ClaudeSDKClient.get_context_usage` | method | `ClaudeClient::get_context_usage` | ported | true | get_context_usage_returns_response_value in tests/client_test.rs. |
| `ClaudeSDKClient.get_server_info` | method | `ClaudeClient::server_info` | ported | true | Cached from the initialize handshake response, not re-fetched (DEVIATIONS.md Phase 7). |
| `ClaudeSDKClient.disconnect` | method | `ClaudeClient::disconnect` | ported | true |  |
| `ClaudeSDKClient (async context manager __aenter__/__aexit__)` | method | `—` | justified_gap | false | Rust has no async-context-manager equivalent to `async with`; callers call connect() then disconnect() explicitly, or rely on Query's best-effort Drop cleanup (DEVIATIONS.md Phase 7) if they skip disconnect() entirely. No capability lost, just no syntactic sugar for the paired call. |

## error

| Upstream symbol | Kind | Rust equivalent | Status | Tested | Notes / Justification |
|---|---|---|---|---|---|
| `claude_agent_sdk.ClaudeSDKError` | error_variant | `Error` | ported | true | The Error enum itself is the base-exception equivalent; all other error entries below are its variants. |
| `claude_agent_sdk.CLIConnectionError` | error_variant | `Error::CliConnection` | ported | true |  |
| `claude_agent_sdk.CLINotFoundError` | error_variant | `Error::CliNotFound` | ported | true | Enriched multi-line message with install hints (DEVIATIONS.md Phase 4). |
| `claude_agent_sdk.ProcessError` | error_variant | `Error::Process` | ported | true | Attaches a real bounded stderr ring buffer, richer than upstream's hardcoded placeholder string (DEVIATIONS.md Phase 4). |
| `claude_agent_sdk.CLIJSONDecodeError` | error_variant | `Error::JsonDecode` | ported | true |  |

## hook

| Upstream symbol | Kind | Rust equivalent | Status | Tested | Notes / Justification |
|---|---|---|---|---|---|
| `claude_agent_sdk.HookCallback` | field | `types::hook::HookCallback` | ported | true |  |
| `claude_agent_sdk.HookContext` | field | `types::hook::HookContext` | ported | true | signal placeholder omitted (unused upstream, future abort-signal support). |
| `claude_agent_sdk.HookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | DEVIATIONS.md Phase 8: hook input stays a raw JSON payload, not a discriminated union type -- every field reachable via serde_json::Value indexing in the same closure a caller writes anyway. No capability lost. |
| `claude_agent_sdk.BaseHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.PreToolUseHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.PostToolUseHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.PostToolUseFailureHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.PostToolUseFailureHookSpecificOutput` | field | `HookOutput.hook_specific_output: Option<serde_json::Value>` | justified_gap | true | hook_specific_output is kept as raw JSON since its shape varies per event -- callers build the exact per-event shape upstream expects; no field-name checking is lost that the caller wasn't already responsible for. |
| `claude_agent_sdk.UserPromptSubmitHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.StopHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.SubagentStopHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.PreCompactHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.NotificationHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.SubagentStartHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.PermissionRequestHookInput` | field | `serde_json::Value (hook callback's first parameter)` | justified_gap | true | Same raw-payload reasoning as HookInput above. |
| `claude_agent_sdk.NotificationHookSpecificOutput` | field | `HookOutput.hook_specific_output: Option<serde_json::Value>` | justified_gap | true | Same raw-JSON reasoning as PostToolUseFailureHookSpecificOutput above. |
| `claude_agent_sdk.SubagentStartHookSpecificOutput` | field | `HookOutput.hook_specific_output: Option<serde_json::Value>` | justified_gap | true | Same raw-JSON reasoning as PostToolUseFailureHookSpecificOutput above. |
| `claude_agent_sdk.PermissionRequestHookSpecificOutput` | field | `HookOutput.hook_specific_output: Option<serde_json::Value>` | justified_gap | true | Same raw-JSON reasoning as PostToolUseFailureHookSpecificOutput above. |
| `claude_agent_sdk.HookJSONOutput` | field | `types::hook::HookOutput` | ported | true | decision/systemMessage/hookSpecificOutput typed; extra fields flatten (async_/continue_ Python keyword-avoidance not needed in Rust). |
| `claude_agent_sdk.HookMatcher` | class | `types::hook::HookMatcher` | ported | true |  |
| `ClaudeAgentOptions.hooks` | field | `ClaudeAgentOptions.hooks: Option<HashMap<HookEvent, Vec<HookMatcher>>>` | ported | true | hook_{i} ids assigned deterministically via ALL_HOOK_EVENTS order, not HashMap iteration (DEVIATIONS.md Phase 8). |
| `HookEvent.PreToolUse` | hook_event | `types::hook::HookEvent::PreToolUse` | ported | true |  |
| `HookEvent.PostToolUse` | hook_event | `types::hook::HookEvent::PostToolUse` | ported | true |  |
| `HookEvent.PostToolUseFailure` | hook_event | `types::hook::HookEvent::PostToolUseFailure` | ported | true | DEVIATIONS.md Phase 8: plan's sketch listed only 6 events, missing this one + 3 others below. |
| `HookEvent.UserPromptSubmit` | hook_event | `types::hook::HookEvent::UserPromptSubmit` | ported | true |  |
| `HookEvent.Stop` | hook_event | `types::hook::HookEvent::Stop` | ported | true |  |
| `HookEvent.SubagentStop` | hook_event | `types::hook::HookEvent::SubagentStop` | ported | true |  |
| `HookEvent.PreCompact` | hook_event | `types::hook::HookEvent::PreCompact` | ported | true |  |
| `HookEvent.Notification` | hook_event | `types::hook::HookEvent::Notification` | ported | true | DEVIATIONS.md Phase 8: plan's sketch missed this event. |
| `HookEvent.SubagentStart` | hook_event | `types::hook::HookEvent::SubagentStart` | ported | true | DEVIATIONS.md Phase 8: plan's sketch missed this event. |
| `HookEvent.PermissionRequest` | hook_event | `types::hook::HookEvent::PermissionRequest` | ported | true | DEVIATIONS.md Phase 8: plan's sketch missed this event. |

## mcp

| Upstream symbol | Kind | Rust equivalent | Status | Tested | Notes / Justification |
|---|---|---|---|---|---|
| `claude_agent_sdk.McpServerConfig` | field | `types::mcp::McpServerConfig` | ported | true | Stdio/Sse/Http/Sdk variants. |
| `claude_agent_sdk.McpSdkServerConfig` | field | `types::mcp::McpServerConfig::Sdk + to_cli_config_json` | ported | true | Stub-serializes to {type:sdk,name:...} with the handler table stripped (DEVIATIONS.md Phase 9). |
| `claude_agent_sdk.McpServerStatus` | field | `ClaudeClient::get_mcp_status -> serde_json::Value` | justified_gap | true | Every field is present and readable in the raw Value; no phase ever scoped a typed wrapper. get_mcp_status()/get_context_usage() return the raw control-response payload as serde_json::Value -- full data access, just not through a dedicated typed struct. Never discussed as a deliberate choice in any phase's DEVIATIONS.md; a compile-time-typed wrapper would be a nice enhancement, not a capability gap. |
| `claude_agent_sdk.McpServerStatusConfig` | field | `ClaudeClient::get_mcp_status -> serde_json::Value` | justified_gap | true | Same raw-Value reasoning as McpServerStatus above. |
| `claude_agent_sdk.McpServerConnectionStatus` | field | `ClaudeClient::get_mcp_status -> serde_json::Value` | justified_gap | true | Same raw-Value reasoning as McpServerStatus above. |
| `claude_agent_sdk.McpServerInfo` | field | `ClaudeClient::get_mcp_status -> serde_json::Value` | justified_gap | true | Distinct from ClaudeClient::server_info(), which caches the initialize handshake response. Same raw-Value reasoning as McpServerStatus above. |
| `claude_agent_sdk.McpStatusResponse` | field | `ClaudeClient::get_mcp_status -> serde_json::Value` | justified_gap | true | Same raw-Value reasoning as McpServerStatus above. |
| `claude_agent_sdk.McpToolAnnotations` | field | `ClaudeClient::get_mcp_status -> serde_json::Value` | justified_gap | true | This is the status-view annotations shape (readOnly/destructive/openWorld), distinct from SdkTool.annotations (the tool-definition input, also a justified_gap below). Same raw-Value reasoning as McpServerStatus above. |
| `claude_agent_sdk.McpToolInfo` | field | `ClaudeClient::get_mcp_status -> serde_json::Value` | justified_gap | true | Same raw-Value reasoning as McpServerStatus above. |
| `claude_agent_sdk.create_sdk_mcp_server` | function | `mcp_server::create_sdk_mcp_server` | ported | true |  |
| `claude_agent_sdk.tool` | function | `mcp_server::tool` | ported | true | Decorator upstream, plain function returning SdkTool here -- Rust has no decorator syntax. |
| `claude_agent_sdk.SdkMcpTool` | class | `mcp_server::SdkTool` | ported | true |  |
| `claude_agent_sdk.ToolAnnotations` | field | `mcp_server::SdkTool.annotations: Option<serde_json::Value>` | justified_gap | true | DEVIATIONS.md Phase 9: kept as raw JSON since ToolAnnotations comes from the external, unvendored `mcp` PyPI package -- forwarded verbatim to tools/list with no capability lost. |
| `SdkMcpServer.handle_message (initialize)` | method | `mcp_server::SdkMcpServer::handle_message` | ported | true | protocolVersion hardcoded to 2024-11-05, not echoed (DEVIATIONS.md Phase 9). |
| `SdkMcpServer.handle_message (notifications/initialized)` | method | `mcp_server::SdkMcpServer::handle_message` | ported | true | Real {jsonrpc,result:{}} response, not "no reply" (DEVIATIONS.md Phase 9). |
| `SdkMcpServer.handle_message (tools/list)` | method | `mcp_server::SdkMcpServer::handle_message` | ported | true | Tools stored in registration-order Vec, not HashMap (DEVIATIONS.md Phase 9). |
| `SdkMcpServer.handle_message (tools/call)` | method | `mcp_server::SdkMcpServer::handle_message` | ported | true | Handler panics caught and converted to a -32603 JSON-RPC error (DEVIATIONS.md Phase 9). |
| `unknown mcp server_name handling` | method | `protocol::query -- McpMessage dispatch arm` | ported | true | JSON-RPC -32601 error INSIDE a success control response, not a control-protocol error (DEVIATIONS.md Phase 9). |

## message_type

| Upstream symbol | Kind | Rust equivalent | Status | Tested | Notes / Justification |
|---|---|---|---|---|---|
| `claude_agent_sdk.UserMessage` | class | `types::message::UserMessage` | ported | true |  |
| `claude_agent_sdk.AssistantMessage` | class | `types::message::AssistantMessage` | ported | true | error kept as Option<String>, not a closed enum (DEVIATIONS.md Phase 2). |
| `claude_agent_sdk.SystemMessage` | class | `types::message::SystemMessage` | ported | true | Generic fallback for subtypes this port doesn't specifically recognize (DEVIATIONS.md Phase 2b). |
| `claude_agent_sdk.TaskStartedMessage` | class | `types::message::TaskStartedMessage` | ported | true | Own top-level Message variant, not a SystemMessage subclass -- Rust has no inheritance (DEVIATIONS.md Phase 2b). |
| `claude_agent_sdk.TaskProgressMessage` | class | `types::message::TaskProgressMessage` | ported | true |  |
| `claude_agent_sdk.TaskUpdatedMessage` | class | `types::message::TaskUpdatedMessage` | ported | true |  |
| `claude_agent_sdk.TaskNotificationMessage` | class | `types::message::TaskNotificationMessage` | ported | true |  |
| `claude_agent_sdk.TaskNotificationStatus` | field | `TaskNotificationMessage.status: String` | justified_gap | true | Values ("completed"/"failed"/"stopped") pass through as a raw string. Kept as a raw String rather than a closed Rust enum, matching the AssistantMessage.error forward-compatibility precedent (DEVIATIONS.md Phase 2) -- the CLI's own value passes through unconstrained. |
| `claude_agent_sdk.TaskUpdatedStatus` | field | `TaskUpdatedMessage.status: Option<String>` | justified_gap | true | Same raw-String reasoning as TaskNotificationStatus above. |
| `claude_agent_sdk.TERMINAL_TASK_STATUSES` | field | `types::message::TERMINAL_TASK_STATUSES` | ported | true |  |
| `claude_agent_sdk.TaskUsage` | field | `types::message::TaskUsage` | ported | true |  |
| `claude_agent_sdk.ResultMessage` | class | `types::message::ResultMessage` | ported | true | All 16 fields ported; see reference_use_case entries for cumulative-cost/per-query-turns proof. |
| `claude_agent_sdk.DeferredToolUse` | field | `types::message::DeferredToolUse` | ported | true |  |
| `claude_agent_sdk.RateLimitEvent` | class | `types::message::RateLimitEvent` | ported | true |  |
| `claude_agent_sdk.RateLimitInfo` | field | `types::message::RateLimitInfo` | ported | true |  |
| `claude_agent_sdk.RateLimitStatus` | field | `RateLimitInfo.status: String` | justified_gap | true | Same raw-String reasoning as TaskNotificationStatus above. |
| `claude_agent_sdk.RateLimitType` | field | `RateLimitInfo.rate_limit_type: Option<String>` | justified_gap | true | Same raw-String reasoning as TaskNotificationStatus above. |
| `claude_agent_sdk.StreamEvent` | class | `types::message::StreamEvent` | ported | true |  |
| `claude_agent_sdk.Message` | field | `types::message::Message` | ported | true | 12 variants: User/Assistant/System/TaskStarted/TaskProgress/TaskNotification/TaskUpdated/MirrorError/HookEvent/Result/StreamEvent/RateLimitEvent. |
| `claude_agent_sdk.TextBlock` | content_block | `types::message::ContentBlock::Text` | ported | true |  |
| `claude_agent_sdk.ThinkingBlock` | content_block | `types::message::ContentBlock::Thinking` | ported | true |  |
| `claude_agent_sdk.ToolUseBlock` | content_block | `types::message::ContentBlock::ToolUse` | ported | true |  |
| `claude_agent_sdk.ToolResultBlock` | content_block | `types::message::ContentBlock::ToolResult` | ported | true |  |
| `claude_agent_sdk.ServerToolName` | field | `ContentBlock::ServerToolUse.name: String` | justified_gap | true | Same raw-String reasoning as TaskNotificationStatus above. |
| `claude_agent_sdk.ServerToolUseBlock` | content_block | `types::message::ContentBlock::ServerToolUse` | ported | true |  |
| `claude_agent_sdk.ServerToolResultBlock` | content_block | `types::message::ContentBlock::ServerToolResult` | ported | true | Wire tag is advisor_tool_result, asymmetric with ServerToolUse's tag (DEVIATIONS.md Phase 2). |
| `claude_agent_sdk.ContentBlock` | field | `types::message::ContentBlock` | ported | true | 6 variants shared across user/assistant roles (DEVIATIONS.md Phase 2 minor simplification). |
| `claude_agent_sdk.HookEventMessage` | class | `types::message::HookEventMessage` | ported | true | Only emitted when include_hook_events is set. |
| `claude_agent_sdk.MirrorErrorMessage` | class | `types::message::MirrorErrorMessage` | ported | true | The message TYPE is ported (Phase 2b) even though the mirror-write path that would emit it (TranscriptMirrorBatcher) is deferred (DEVIATIONS.md Phase 5). |

## options_field

| Upstream symbol | Kind | Rust equivalent | Status | Tested | Notes / Justification |
|---|---|---|---|---|---|
| `claude_agent_sdk.EffortLevel` | field | `types::options::EffortLevel` | ported | true |  |
| `claude_agent_sdk.ClaudeAgentOptions` | class | `types::options::ClaudeAgentOptions` | ported | true | See per-field options_field entries below. |
| `claude_agent_sdk.TaskBudget` | field | `types::options::TaskBudget` | ported | true |  |
| `claude_agent_sdk.ThinkingConfig` | field | `types::options::ThinkingConfig` | ported | true |  |
| `claude_agent_sdk.ThinkingConfigAdaptive` | field | `types::options::ThinkingConfig::Adaptive` | ported | true |  |
| `claude_agent_sdk.ThinkingConfigEnabled` | field | `types::options::ThinkingConfig::Enabled` | ported | true |  |
| `claude_agent_sdk.ThinkingConfigDisabled` | field | `types::options::ThinkingConfig::Disabled` | ported | true |  |
| `claude_agent_sdk.AgentDefinition` | class | `types::options::AgentDefinition` | ported | true | Full 12 fields (DEVIATIONS.md Phase 3). |
| `claude_agent_sdk.SettingSource` | field | `types::options::SettingSource` | ported | true |  |
| `claude_agent_sdk.SdkPluginConfig` | field | `types::mcp::PluginConfig` | ported | true | Only the local variant exists upstream (DEVIATIONS.md Phase 3). |
| `claude_agent_sdk.SessionKey` | field | `types::session_store::SessionKey` | ported | true | The SessionStore trait + its data types ARE ported (Phase 3) -- only the listing/mutation functions built on top are missing. |
| `claude_agent_sdk.SessionStore` | class | `types::session_store::SessionStore` | ported | true | Hand-rolled boxed-future trait methods, not async-trait, per the crate's dependency policy (DEVIATIONS.md Phase 3). |
| `claude_agent_sdk.SessionStoreEntry` | field | `types::session_store::SessionStoreEntry` | ported | true |  |
| `claude_agent_sdk.SessionStoreFlushMode` | field | `types::session_store::SessionStoreFlushMode` | ported | true |  |
| `claude_agent_sdk.SessionStoreListEntry` | field | `types::session_store::SessionStoreListEntry` | ported | true |  |
| `claude_agent_sdk.SessionSummaryEntry` | field | `types::session_store::SessionSummaryEntry` | ported | true |  |
| `claude_agent_sdk.SessionListSubkeysKey` | field | `types::session_store::SessionListSubkeysKey` | ported | true |  |
| `claude_agent_sdk.SdkBeta` | field | `String (ClaudeAgentOptions.betas: Vec<String>)` | ported | true |  |
| `claude_agent_sdk.SandboxSettings` | field | `types::options::SandboxSettings` | ported | true |  |
| `claude_agent_sdk.SandboxNetworkConfig` | field | `types::options::SandboxNetworkConfig` | ported | true |  |
| `claude_agent_sdk.SandboxIgnoreViolations` | field | `types::options::SandboxIgnoreViolations` | ported | true |  |
| `ClaudeAgentOptions.tools` | field | `ClaudeAgentOptions.tools: Option<ToolsOption>` | ported | true |  |
| `ClaudeAgentOptions.allowed_tools` | field | `ClaudeAgentOptions.allowed_tools: Vec<String>` | ported | true |  |
| `ClaudeAgentOptions.system_prompt` | field | `ClaudeAgentOptions.system_prompt: Option<SystemPrompt>` | ported | true | Custom/Preset(+append,+exclude_dynamic_sections)/File variants; unset ALWAYS emits --system-prompt "" (DEVIATIONS.md Phase 3). See reference_use_case entry for the claude_code preset+append proof. |
| `ClaudeAgentOptions.mcp_servers` | field | `ClaudeAgentOptions.mcp_servers: McpServersOption` | ported | true | Dict or bare path/inline-JSON string (DEVIATIONS.md Phase 3). |
| `ClaudeAgentOptions.strict_mcp_config` | field | `ClaudeAgentOptions.strict_mcp_config: bool` | ported | true |  |
| `ClaudeAgentOptions.permission_mode` | field | `ClaudeAgentOptions.permission_mode: Option<PermissionMode>` | ported | true | See reference_use_case entry for bypassPermissions proof. |
| `ClaudeAgentOptions.continue_conversation` | field | `ClaudeAgentOptions.continue_conversation: bool` | ported | true |  |
| `ClaudeAgentOptions.resume` | field | `ClaudeAgentOptions.resume: Option<String>` | ported | true | Truthy CLI-arg check: Some(String::new()) omits the flag (DEVIATIONS.md Phase 3). See reference_use_case entry. |
| `ClaudeAgentOptions.session_id` | field | `ClaudeAgentOptions.session_id: Option<String>` | ported | true |  |
| `ClaudeAgentOptions.max_turns` | field | `ClaudeAgentOptions.max_turns: Option<u32>` | ported | true | max_turns_zero_omits_flag locks in the truthy-check quirk (DEVIATIONS.md Phase 3). See reference_use_case entry. |
| `ClaudeAgentOptions.max_budget_usd` | field | `ClaudeAgentOptions.max_budget_usd: Option<f64>` | ported | true | is_not_None check: 0.0 still emits the flag (DEVIATIONS.md Phase 3). |
| `ClaudeAgentOptions.disallowed_tools` | field | `ClaudeAgentOptions.disallowed_tools: Vec<String>` | ported | true |  |
| `ClaudeAgentOptions.model` | field | `ClaudeAgentOptions.model: Option<String>` | ported | true | See reference_use_case entry. |
| `ClaudeAgentOptions.fallback_model` | field | `ClaudeAgentOptions.fallback_model: Option<String>` | ported | true |  |
| `ClaudeAgentOptions.betas` | field | `ClaudeAgentOptions.betas: Vec<String>` | ported | true |  |
| `ClaudeAgentOptions.permission_prompt_tool_name` | field | `ClaudeAgentOptions.permission_prompt_tool_name: Option<String>` | ported | true | Auto-set to "stdio" when can_use_tool is set (DEVIATIONS.md Phase 8). |
| `ClaudeAgentOptions.cwd` | field | `ClaudeAgentOptions.cwd: Option<PathBuf>` | ported | true | See reference_use_case entry. |
| `ClaudeAgentOptions.cli_path` | field | `ClaudeAgentOptions.cli_path: Option<PathBuf>` | ported | true |  |
| `ClaudeAgentOptions.settings` | field | `ClaudeAgentOptions.settings: Option<String>` | ported | true | See reference_use_case entry for file-path + inline-JSON-string proof. |
| `ClaudeAgentOptions.add_dirs` | field | `ClaudeAgentOptions.add_dirs: Vec<PathBuf>` | ported | true | See reference_use_case entry. |
| `ClaudeAgentOptions.env` | field | `ClaudeAgentOptions.env: HashMap<String, String>` | ported | true |  |
| `ClaudeAgentOptions.extra_args` | field | `ClaudeAgentOptions.extra_args: HashMap<String, Option<String>>` | ported | true |  |
| `ClaudeAgentOptions.max_buffer_size` | field | `ClaudeAgentOptions.max_buffer_size: Option<usize>` | ported | true |  |
| `ClaudeAgentOptions.debug_stderr` | field | `—` | justified_gap | false | DEVIATIONS.md Phase 3: upstream itself documents this field as "Deprecated and no longer read by the transport" -- a dead field, not a gap. |
| `ClaudeAgentOptions.stderr` | field | `ClaudeAgentOptions.stderr: Option<StderrCallback>` | ported | true | See reference_use_case entry for every-line delivery proof. |
| `ClaudeAgentOptions.user` | field | `ClaudeAgentOptions.user: Option<String>` | ported | true | Not a CLI flag -- an OS-level subprocess-spawn parameter, consumed by the transport (DEVIATIONS.md Phase 3). |
| `ClaudeAgentOptions.include_partial_messages` | field | `ClaudeAgentOptions.include_partial_messages: bool` | ported | true | See reference_use_case entry. |
| `ClaudeAgentOptions.include_hook_events` | field | `ClaudeAgentOptions.include_hook_events: bool` | ported | true |  |
| `ClaudeAgentOptions.fork_session` | field | `ClaudeAgentOptions.fork_session: bool` | ported | true |  |
| `ClaudeAgentOptions.agents` | field | `ClaudeAgentOptions.agents: Option<HashMap<String, AgentDefinition>>` | ported | true | No CLI flag -- delivered via the initialize control request (DEVIATIONS.md Phase 3). |
| `ClaudeAgentOptions.setting_sources` | field | `ClaudeAgentOptions.setting_sources: Option<Vec<SettingSource>>` | ported | true | Single =-joined --setting-sources=a,b,c argument (DEVIATIONS.md Phase 3). |
| `ClaudeAgentOptions.skills` | field | `ClaudeAgentOptions.skills: Option<SkillsOption>` | ported | true | Not its own CLI flag -- injects Skill/Skill(name) into allowed_tools (DEVIATIONS.md Phase 3). |
| `ClaudeAgentOptions.sandbox` | field | `ClaudeAgentOptions.sandbox: Option<SandboxSettings>` | ported | true |  |
| `ClaudeAgentOptions.plugins` | field | `ClaudeAgentOptions.plugins: Vec<PluginConfig>` | ported | true | Repeated --plugin-dir <path> per local plugin, not a single JSON flag (DEVIATIONS.md Phase 3). See reference_use_case entry. |
| `ClaudeAgentOptions.max_thinking_tokens` | field | `ClaudeAgentOptions.max_thinking_tokens: Option<u32>` | ported | true |  |
| `ClaudeAgentOptions.thinking` | field | `ClaudeAgentOptions.thinking: Option<ThinkingConfig>` | ported | true | Takes precedence over deprecated max_thinking_tokens when both set. |
| `ClaudeAgentOptions.effort` | field | `ClaudeAgentOptions.effort: Option<EffortLevel>` | ported | true |  |
| `ClaudeAgentOptions.output_format` | field | `ClaudeAgentOptions.output_format: Option<serde_json::Value>` | ported | true |  |
| `ClaudeAgentOptions.enable_file_checkpointing` | field | `ClaudeAgentOptions.enable_file_checkpointing: bool` | ported | true |  |
| `ClaudeAgentOptions.session_store` | field | `ClaudeAgentOptions.session_store: Option<Arc<dyn SessionStore>>` | ported | true | Accepted as a value; the load/mirror machinery that would consume it is deferred (TranscriptMirrorBatcher, DEVIATIONS.md Phase 5/6). |
| `ClaudeAgentOptions.session_store_flush` | field | `ClaudeAgentOptions.session_store_flush: SessionStoreFlushMode` | ported | true |  |
| `ClaudeAgentOptions.load_timeout_ms` | field | `ClaudeAgentOptions.load_timeout_ms: u64` | ported | true |  |
| `ClaudeAgentOptions.task_budget` | field | `ClaudeAgentOptions.task_budget: Option<TaskBudget>` | ported | true |  |

## permission

| Upstream symbol | Kind | Rust equivalent | Status | Tested | Notes / Justification |
|---|---|---|---|---|---|
| `claude_agent_sdk.PermissionMode` | field | `types::permission::PermissionMode` | ported | true | 6 variants: default/acceptEdits/plan/bypassPermissions/dontAsk/auto. |
| `claude_agent_sdk.CanUseTool` | field | `types::permission::CanUseToolCallback` | ported | true |  |
| `claude_agent_sdk.CanUseToolShadowedWarning` | class | `tracing::warn! in callback_adapters::warn_if_can_use_tool_shadowed` | justified_gap | true | CanUseToolShadowedWarning is a Python UserWarning subclass; this crate has no exception/warning class hierarchy. The same advisory fires via tracing::warn! with identical trigger conditions and message content (DEVIATIONS.md Phase 8) -- a delivery-mechanism adaptation, not a lost capability. |
| `claude_agent_sdk.ToolPermissionContext` | field | `types::permission::ToolPermissionRequest` | ported | true | All 8 fields (suggestions/tool_use_id/agent_id/blocked_path/decision_reason/title/display_name/description); signal omitted (unused upstream placeholder). |
| `claude_agent_sdk.PermissionResult` | field | `types::permission::PermissionResult` | ported | true |  |
| `claude_agent_sdk.PermissionResultAllow` | class | `types::permission::PermissionResult::Allow` | ported | true |  |
| `claude_agent_sdk.PermissionResultDeny` | class | `types::permission::PermissionResult::Deny` | ported | true |  |
| `claude_agent_sdk.PermissionUpdate` | class | `types::permission::PermissionUpdate` | ported | true | See per-variant permission entries below. |
| `ClaudeAgentOptions.can_use_tool` | field | `ClaudeAgentOptions.can_use_tool: Option<CanUseToolCallback>` | ported | true | Mutual-exclusivity validation with string-prompt and permission_prompt_tool_name implemented (DEVIATIONS.md Phase 8). |
| `PermissionUpdate.addRules` | field | `types::permission::PermissionUpdate::AddRules` | ported | true |  |
| `PermissionUpdate.replaceRules` | field | `types::permission::PermissionUpdate::ReplaceRules` | ported | true |  |
| `PermissionUpdate.removeRules` | field | `types::permission::PermissionUpdate::RemoveRules` | ported | true |  |
| `PermissionUpdate.setMode` | field | `types::permission::PermissionUpdate::SetMode` | ported | true |  |
| `PermissionUpdate.addDirectories` | field | `types::permission::PermissionUpdate::AddDirectories` | ported | true |  |
| `PermissionUpdate.removeDirectories` | field | `types::permission::PermissionUpdate::RemoveDirectories` | ported | true |  |

## public_api

| Upstream symbol | Kind | Rust equivalent | Status | Tested | Notes / Justification |
|---|---|---|---|---|---|
| `claude_agent_sdk.query` | function | `query::query` | ported | true | One-shot query; always initializes, always streaming input (DEVIATIONS.md Phase 4/6). |
| `claude_agent_sdk.__version__` | field | `—` | justified_gap | false | Cargo.toml's own `version` field is the Rust-idiomatic equivalent of a runtime __version__ constant; no runtime constant needed in a compiled crate. |
| `claude_agent_sdk.Transport` | class | `transport::Transport` | ported | true | Trait, not a class; ClaudeClient::connect_with_transport accepts any impl Transport. |
| `claude_agent_sdk.ClaudeSDKClient` | class | `client::ClaudeClient` | ported | true | See client_method entries below for the 16 upstream + connect_with_transport addition. |
| `claude_agent_sdk.list_sessions` | function | `—` | not_ported | false | Session listing/query/mutation subsystem has no Rust equivalent anywhere in this port. No phase in the 10-phase plan scoped it (Phase 3 added the SessionStore trait + data types but never the functions built on top of it). Not exercised by refiner/foreman/prisma. Needs an owner decision: new phase, or explicit scope exclusion. |
| `claude_agent_sdk.get_session_info` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.get_session_messages` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.list_subagents` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.get_subagent_messages` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.SDKSessionInfo` | field | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.SessionMessage` | field | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.InMemorySessionStore` | class | `—` | not_ported | false | A reference SessionStore implementation upstream ships for tests/examples. This port defines the trait (Phase 3) but ships no in-memory implementation. See list_sessions entry for the broader subsystem gap. |
| `claude_agent_sdk.fold_session_summary` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.project_key_for_directory` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.import_session_to_store` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.list_sessions_from_store` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.get_session_info_from_store` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.get_session_messages_from_store` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.list_subagents_from_store` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.get_subagent_messages_from_store` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.rename_session` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.tag_session` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.delete_session` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.fork_session` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.ForkSessionResult` | field | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.rename_session_via_store` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.tag_session_via_store` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.delete_session_via_store` | function | `—` | not_ported | false | See list_sessions entry. |
| `claude_agent_sdk.fork_session_via_store` | function | `—` | not_ported | false | See list_sessions entry. |

## reference_use_case

| Upstream symbol | Kind | Rust equivalent | Status | Tested | Notes / Justification |
|---|---|---|---|---|---|
| `refiner.SDKWrapper: cumulative total_cost_usd, per-query num_turns` | field | `ResultMessage.total_cost_usd / ResultMessage.num_turns` | ported | true | refiner computes its own per-query delta (cumulative_cost - cost_before) from the CLI's own running total; the Rust port surfaces both raw fields identically across two ResultMessages on one ClaudeClient session. |
| `refiner/foreman: stderr callback delivering every CLI stderr line` | field | `ClaudeAgentOptions.stderr: Option<StderrCallback>` | ported | true | refiner/foreman keep a bounded ring buffer of the last 50 lines fed by this callback for error-detail enrichment; transport_test.rs's stderr_callback_receives_each_line proves every line reaches the callback. |
| `foreman: plugins option reaching the CLI invocation` | field | `ClaudeAgentOptions.plugins: Vec<PluginConfig>` | ported | true | options.rs's plugins_serialize_into_plugin_dir_flag test proves repeated --plugin-dir <path> flags reach the CLI invocation. |
| `refiner: system_prompt preset claude_code + append` | field | `ClaudeAgentOptions.system_prompt: SystemPrompt::Preset { append, .. }` | ported | true | options.rs's system_prompt_custom_vs_preset_append test proves the exact wire form. |
| `foreman/prisma: settings accepting a file path and an inline JSON string` | field | `ClaudeAgentOptions.settings: Option<String>` | ported | true | settings is a single Option<String> that reaches the CLI's --settings flag verbatim -- both foreman (file path) and a raw inline-JSON value pass through identically since the flag argument is unconstrained by this port. |
| `refiner/prisma: ResultMessage field readability` | field | `types::message::ResultMessage` | ported | true | subtype, num_turns, total_cost_usd, duration_ms, duration_api_ms, is_error, session_id, result, usage.input_tokens/usage.output_tokens (via serde_json::Value indexing on usage) all present and read by both wrappers' metrics logging. |
| `refiner: resume (resume_session_id)` | field | `ClaudeAgentOptions.resume: Option<String>` | ported | true |  |
| `refiner: add_dirs` | field | `ClaudeAgentOptions.add_dirs: Vec<PathBuf>` | ported | true |  |
| `refiner/foreman: include_partial_messages` | field | `ClaudeAgentOptions.include_partial_messages: bool` | ported | true |  |
| `refiner: max_turns` | field | `ClaudeAgentOptions.max_turns: Option<u32>` | ported | true |  |
| `prisma: allowed_tools (empty list to disable all tools)` | field | `ClaudeAgentOptions.allowed_tools: Vec<String>` | ported | true |  |
| `refiner/foreman/prisma: permission_mode bypassPermissions` | field | `ClaudeAgentOptions.permission_mode: Option<PermissionMode>` | ported | true |  |
| `refiner/prisma: cwd` | field | `ClaudeAgentOptions.cwd: Option<PathBuf>` | ported | true |  |
| `refiner/prisma: model` | field | `ClaudeAgentOptions.model: Option<String>` | ported | true |  |
| `refiner/foreman/prisma: full connect -> query -> typed message loop -> metrics -> disconnect core loop` | method | `examples/reference_wrapper.rs` | ported | true | Compiling Rust translation of refiner's SDKWrapper core loop: connect, send, typed message loop with tool-call printing, ResultMessage metrics, disconnect, stderr ring buffer. Every line of refiner's core loop is expressible in the current public API. |

