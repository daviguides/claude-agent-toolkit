# Appendix A — Wire Protocol Samples (Test Fixtures)

**Purpose**: seed JSON for `tests/fixtures/`. Every sample here is a
best-effort sketch written from knowledge of the upstream SDK.

**⚠️ MANDATORY VERIFICATION STEP**: before using any sample as a
fixture, cross-check it against the pinned upstream clone:

- Parsing rules: `src/claude_agent_sdk/_internal/message_parser.py`
- Control protocol: `src/claude_agent_sdk/_internal/query.py`
- Real captured shapes: `reference/.../tests/` (upstream's own test
  suite contains authoritative message samples — prefer copying those
  verbatim over the sketches below)

When a sketch and upstream disagree, upstream wins; fix the fixture AND
note the difference in `DEVIATIONS.md` so the plan stays honest.

## A.1 Messages: CLI stdout → SDK

`assistant_text.json`
```json
{"type":"assistant","message":{"model":"claude-sonnet-5","content":[{"type":"text","text":"Hello! 2 + 2 = 4."}]},"parent_tool_use_id":null,"session_id":"sess_123"}
```

`assistant_tool_use.json`
```json
{"type":"assistant","message":{"model":"claude-sonnet-5","content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/tmp/x.txt"}}]},"parent_tool_use_id":null,"session_id":"sess_123"}
```

`assistant_thinking.json`
```json
{"type":"assistant","message":{"model":"claude-sonnet-5","content":[{"type":"thinking","thinking":"Let me consider...","signature":"EqQBCg=="}]},"parent_tool_use_id":null,"session_id":"sess_123"}
```

`user_text.json`
```json
{"type":"user","message":{"role":"user","content":"What is 2 + 2?"},"parent_tool_use_id":null,"session_id":"sess_123"}
```

`user_blocks.json` (tool result flowing back through a user message)
```json
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01","content":"file contents here","is_error":false}]},"parent_tool_use_id":null,"session_id":"sess_123"}
```

`system_init.json` (system messages carry many extra fields — keep raw)
```json
{"type":"system","subtype":"init","cwd":"/home/user/project","session_id":"sess_123","tools":["Read","Bash"],"model":"claude-sonnet-5","permissionMode":"default","apiKeySource":"none"}
```

`result_success.json`
```json
{"type":"result","subtype":"success","duration_ms":2threshold,"duration_api_ms":1800,"is_error":false,"num_turns":1,"session_id":"sess_123","total_cost_usd":0.0031,"usage":{"input_tokens":10,"output_tokens":20},"result":"2 + 2 = 4."}
```
(NOTE: the `2threshold` above is intentionally broken so the executor
cannot skip verification — replace with a real integer, e.g. `2400`,
after checking upstream's result fixture.)

`result_minimal.json` (optional fields absent)
```json
{"type":"result","subtype":"error_during_execution","duration_ms":100,"duration_api_ms":0,"is_error":true,"num_turns":0,"session_id":"sess_123"}
```

`stream_event.json`
```json
{"type":"stream_event","uuid":"evt_1","session_id":"sess_123","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}},"parent_tool_use_id":null}
```

## A.2 SDK stdin → CLI (streaming mode)

User message:
```json
{"type":"user","message":{"role":"user","content":"hello"},"parent_tool_use_id":null,"session_id":"default"}
```

## A.3 Control protocol — SDK-initiated

Interrupt:
```json
{"type":"control_request","request_id":"req_1_test","request":{"subtype":"interrupt"}}
```

Set permission mode:
```json
{"type":"control_request","request_id":"req_2_test","request":{"subtype":"set_permission_mode","mode":"acceptEdits"}}
```

Set model:
```json
{"type":"control_request","request_id":"req_3_test","request":{"subtype":"set_model","model":"claude-opus-4-8"}}
```

Initialize (with one hook registered):
```json
{"type":"control_request","request_id":"req_0_test","request":{"subtype":"initialize","hooks":{"PreToolUse":[{"matcher":"Bash","hookCallbackIds":["hook_0"]}]}}}
```

Success response (CLI → SDK):
```json
{"type":"control_response","response":{"subtype":"success","request_id":"req_1_test","response":{}}}
```

Error response (CLI → SDK):
```json
{"type":"control_response","response":{"subtype":"error","request_id":"req_1_test","error":"interrupt not available"}}
```

## A.4 Control protocol — CLI-initiated

Permission request:
```json
{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"},"permission_suggestions":null}}
```

SDK allow response:
```json
{"type":"control_response","response":{"subtype":"success","request_id":"cli_req_1","response":{"behavior":"allow","updatedInput":{"command":"ls"}}}}
```

SDK deny response:
```json
{"type":"control_response","response":{"subtype":"success","request_id":"cli_req_1","response":{"behavior":"deny","message":"Bash is not allowed","interrupt":false}}}
```

Hook callback request:
```json
{"type":"control_request","request_id":"cli_req_2","request":{"subtype":"hook_callback","callback_id":"hook_0","input":{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls"}}}}
```

SDK hook response (empty = no opinion):
```json
{"type":"control_response","response":{"subtype":"success","request_id":"cli_req_2","response":{}}}
```

MCP message request (tools/list):
```json
{"type":"control_request","request_id":"cli_req_3","request":{"subtype":"mcp_message","server_name":"calc","message":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}}}
```

SDK MCP response (⚠️ VERIFY envelope key name in `_internal/query.py`):
```json
{"type":"control_response","response":{"subtype":"success","request_id":"cli_req_3","response":{"mcp_response":{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"add","description":"Adds two numbers","inputSchema":{"type":"object","properties":{"a":{"type":"number"},"b":{"type":"number"}},"required":["a","b"]}}]}}}}}
```

## A.5 CLI invocation cheat-sheet (⚠️ VERIFY against `_build_command()`)

One-shot:
```
claude --output-format stream-json --verbose [option flags...] --print "<prompt>"
```

Streaming:
```
claude --output-format stream-json --verbose [option flags...] --input-format stream-json
```

Environment: parent env + user `options.env` +
`CLAUDE_CODE_ENTRYPOINT=sdk-rust`.
