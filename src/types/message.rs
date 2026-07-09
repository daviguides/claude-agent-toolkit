//! Typed messages emitted by the Claude Code CLI.

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    fn fixture(name: &str) -> Value {
        let raw = match name {
            "assistant_text" => include_str!("../../tests/fixtures/assistant_text.json"),
            "assistant_tool_use" => {
                include_str!("../../tests/fixtures/assistant_tool_use.json")
            }
            "assistant_thinking" => {
                include_str!("../../tests/fixtures/assistant_thinking.json")
            }
            "assistant_server_tool_use" => {
                include_str!("../../tests/fixtures/assistant_server_tool_use.json")
            }
            "assistant_server_tool_result" => {
                include_str!("../../tests/fixtures/assistant_server_tool_result.json")
            }
            "assistant_full_fields" => {
                include_str!("../../tests/fixtures/assistant_full_fields.json")
            }
            "user_text" => include_str!("../../tests/fixtures/user_text.json"),
            "user_blocks" => include_str!("../../tests/fixtures/user_blocks.json"),
            "user_with_uuid" => include_str!("../../tests/fixtures/user_with_uuid.json"),
            "system_init" => include_str!("../../tests/fixtures/system_init.json"),
            "result_success" => include_str!("../../tests/fixtures/result_success.json"),
            "result_minimal" => include_str!("../../tests/fixtures/result_minimal.json"),
            "result_full" => include_str!("../../tests/fixtures/result_full.json"),
            "stream_event" => include_str!("../../tests/fixtures/stream_event.json"),
            other => panic!("unknown fixture: {other}"),
        };
        serde_json::from_str(raw).expect("fixture is valid JSON")
    }

    #[test]
    fn parses_assistant_text_message() {
        let message = parse_message(fixture("assistant_text"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        assert!(!assistant.model.is_empty());
        assert_eq!(assistant.content.len(), 1);
        assert!(matches!(&assistant.content[0], ContentBlock::Text { text } if !text.is_empty()));
    }

    #[test]
    fn parses_assistant_tool_use_message() {
        let message = parse_message(fixture("assistant_tool_use"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        let ContentBlock::ToolUse { id, name, input } = &assistant.content[0] else {
            panic!("expected ContentBlock::ToolUse");
        };
        assert_eq!(id, "toolu_01");
        assert_eq!(name, "Read");
        assert_eq!(input["file_path"], "/tmp/x.txt");
    }

    #[test]
    fn parses_thinking_block() {
        let message = parse_message(fixture("assistant_thinking"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        let ContentBlock::Thinking { thinking, signature } = &assistant.content[0] else {
            panic!("expected ContentBlock::Thinking");
        };
        assert_eq!(thinking, "Let me consider...");
        assert_eq!(signature, "EqQBCg==");
    }

    #[test]
    fn parses_assistant_server_tool_use_block() {
        let message = parse_message(fixture("assistant_server_tool_use"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        let ContentBlock::ServerToolUse { id, name, .. } = &assistant.content[0] else {
            panic!("expected ContentBlock::ServerToolUse");
        };
        assert_eq!(id, "srvtoolu_01ABC");
        assert_eq!(name, "advisor");
    }

    #[test]
    fn parses_assistant_server_tool_result_block() {
        let message = parse_message(fixture("assistant_server_tool_result"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        let ContentBlock::ServerToolResult { tool_use_id, content } = &assistant.content[0]
        else {
            panic!("expected ContentBlock::ServerToolResult");
        };
        assert_eq!(tool_use_id, "srvtoolu_01ABC");
        assert_eq!(content["type"], "advisor_result");
    }

    #[test]
    fn parses_assistant_message_with_all_fields() {
        let message = parse_message(fixture("assistant_full_fields"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        assert_eq!(
            assistant.message_id.as_deref(),
            Some("msg_01HRq7YZE3apPqSHydvG77Ve")
        );
        assert_eq!(assistant.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(
            assistant.session_id.as_deref(),
            Some("fdf2d90a-fd9e-4736-ae35-806edd13643f")
        );
        assert_eq!(
            assistant.uuid.as_deref(),
            Some("0dbd2453-1209-4fe9-bd51-4102f64e33df")
        );
        assert!(assistant.usage.is_some());
        assert!(assistant.error.is_none());
    }

    #[test]
    fn parses_tool_result_block_with_error_flag() {
        let message = parse_message(fixture("user_blocks"))
            .expect("parses")
            .expect("is Some");
        let Message::User(user) = message else {
            panic!("expected Message::User");
        };
        let UserContent::Blocks(blocks) = user.content else {
            panic!("expected UserContent::Blocks");
        };
        let ContentBlock::ToolResult {
            tool_use_id,
            is_error,
            ..
        } = &blocks[0]
        else {
            panic!("expected ContentBlock::ToolResult");
        };
        assert_eq!(tool_use_id, "toolu_01");
        assert_eq!(*is_error, Some(false));
    }

    #[test]
    fn parses_user_string_content() {
        let message = parse_message(fixture("user_text"))
            .expect("parses")
            .expect("is Some");
        let Message::User(user) = message else {
            panic!("expected Message::User");
        };
        assert!(matches!(user.content, UserContent::Text(text) if text == "What is 2 + 2?"));
    }

    #[test]
    fn parses_user_block_content() {
        let message = parse_message(fixture("user_blocks"))
            .expect("parses")
            .expect("is Some");
        let Message::User(user) = message else {
            panic!("expected Message::User");
        };
        assert!(matches!(user.content, UserContent::Blocks(_)));
    }

    #[test]
    fn parses_user_message_with_uuid() {
        let message = parse_message(fixture("user_with_uuid"))
            .expect("parses")
            .expect("is Some");
        let Message::User(user) = message else {
            panic!("expected Message::User");
        };
        assert_eq!(user.uuid.as_deref(), Some("msg-abc123-def456"));
    }

    #[test]
    fn parses_system_message_keeps_raw_data() {
        let message = parse_message(fixture("system_init"))
            .expect("parses")
            .expect("is Some");
        let Message::System(system) = message else {
            panic!("expected Message::System");
        };
        assert_eq!(system.subtype, "init");
        assert_eq!(system.data["cwd"], "/home/user/project");
        assert_eq!(system.data["tools"][0], "Read");
    }

    #[test]
    fn parses_result_message_full() {
        let message = parse_message(fixture("result_success"))
            .expect("parses")
            .expect("is Some");
        let Message::Result(result) = message else {
            panic!("expected Message::Result");
        };
        assert_eq!(result.subtype, "success");
        assert_eq!(result.duration_ms, 2400);
        assert_eq!(result.duration_api_ms, 1800);
        assert!(!result.is_error);
        assert_eq!(result.num_turns, 1);
        assert_eq!(result.session_id, "sess_123");
        assert_eq!(result.total_cost_usd, Some(0.0031));
        assert!(result.usage.is_some());
        assert_eq!(result.result.as_deref(), Some("2 + 2 = 4."));
    }

    #[test]
    fn parses_result_message_without_optional_fields() {
        let message = parse_message(fixture("result_minimal"))
            .expect("parses")
            .expect("is Some");
        let Message::Result(result) = message else {
            panic!("expected Message::Result");
        };
        assert_eq!(result.total_cost_usd, None);
        assert_eq!(result.usage, None);
        assert_eq!(result.result, None);
        assert_eq!(result.model_usage, None);
        assert_eq!(result.deferred_tool_use, None);
    }

    #[test]
    fn parses_result_message_with_extended_fields() {
        let message = parse_message(fixture("result_full"))
            .expect("parses")
            .expect("is Some");
        let Message::Result(result) = message else {
            panic!("expected Message::Result");
        };
        let model_usage = result.model_usage.expect("model_usage present");
        assert_eq!(
            model_usage["claude-sonnet-4-5-20250929"]["costUSD"],
            0.0106
        );
        assert_eq!(result.permission_denials, Some(Vec::new()));
        assert_eq!(
            result.uuid.as_deref(),
            Some("d379c496-f33a-4ea4-b920-3c5483baa6f7")
        );
    }

    #[test]
    fn parses_stream_event() {
        let message = parse_message(fixture("stream_event"))
            .expect("parses")
            .expect("is Some");
        let Message::StreamEvent(event) = message else {
            panic!("expected Message::StreamEvent");
        };
        assert_eq!(event.uuid, "evt_1");
        assert_eq!(event.session_id, "sess_123");
        assert_eq!(event.event["type"], "content_block_delta");
    }

    #[test]
    fn skips_unknown_message_type() {
        let result = parse_message(serde_json::json!({"type": "rate_limit_event"}));
        assert_eq!(result.expect("does not error"), None);
    }

    #[test]
    fn rejects_message_without_type() {
        let err = parse_message(serde_json::json!({})).expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn rejects_non_object_data() {
        let err = parse_message(serde_json::json!("not an object")).expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn rejects_assistant_missing_content() {
        let err = parse_message(serde_json::json!({"type": "assistant"})).expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn rejects_user_missing_message() {
        let err = parse_message(serde_json::json!({"type": "user"})).expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn rejects_assistant_string_content() {
        let err = parse_message(serde_json::json!({
            "type": "assistant",
            "message": {"model": "m", "content": "hi"}
        }))
        .expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn rejects_non_object_content_block() {
        let err = parse_message(serde_json::json!({
            "type": "assistant",
            "message": {"model": "m", "content": ["oops"]}
        }))
        .expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn skips_unknown_content_block_type() {
        let message = parse_message(serde_json::json!({
            "type": "assistant",
            "message": {
                "model": "m",
                "content": [
                    {"type": "text", "text": "kept"},
                    {"type": "future_block", "whatever": 1}
                ]
            }
        }))
        .expect("parses")
        .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        assert_eq!(assistant.content.len(), 1);
    }

    #[test]
    fn rejects_known_block_type_missing_required_field() {
        let err = parse_message(serde_json::json!({
            "type": "assistant",
            "message": {
                "model": "m",
                "content": [{"type": "text"}]
            }
        }))
        .expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn tolerates_unknown_extra_fields() {
        let mut raw = fixture("assistant_text");
        raw.as_object_mut()
            .expect("object")
            .insert("future_field".to_string(), serde_json::json!(1));
        let message = parse_message(raw).expect("parses").expect("is Some");
        assert!(matches!(message, Message::Assistant(_)));
    }

    #[rstest]
    #[case(ContentBlock::Text { text: "hi".to_string() })]
    #[case(ContentBlock::Thinking { thinking: "t".to_string(), signature: "s".to_string() })]
    #[case(ContentBlock::ToolUse { id: "1".to_string(), name: "Read".to_string(), input: serde_json::json!({}) })]
    #[case(ContentBlock::ToolResult { tool_use_id: "1".to_string(), content: Some(serde_json::json!("ok")), is_error: Some(false) })]
    #[case(ContentBlock::ServerToolUse { id: "1".to_string(), name: "advisor".to_string(), input: serde_json::json!({}) })]
    #[case(ContentBlock::ServerToolResult { tool_use_id: "1".to_string(), content: serde_json::json!({}) })]
    fn content_block_roundtrip(#[case] block: ContentBlock) {
        let json = serde_json::to_value(&block).expect("serializes");
        let parsed: ContentBlock = serde_json::from_value(json).expect("deserializes");
        assert_eq!(parsed, block);
    }
}
