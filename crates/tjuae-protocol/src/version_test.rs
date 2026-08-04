use crate::events::{Capabilities, ProtocolEvent};
use crate::version::JSON_STREAM_PROTOCOL_VERSION;
use tjuae_types::message::ImageInputCapability;

#[test]
fn ready_event_keeps_protocol_0_2_contract() {
    let event = ProtocolEvent::Ready {
        version: JSON_STREAM_PROTOCOL_VERSION.to_string(),
        session_id: None,
        capabilities: Capabilities {
            tool_approval: true,
            image_input: ImageInputCapability::Unknown,
            thinking: false,
            effort: false,
            effort_levels: Vec::new(),
            modes: vec!["default".to_string()],
            current_mode: "default".to_string(),
            mcp: false,
        },
    };

    let value = serde_json::to_value(event).expect("ready event should serialize");

    assert_eq!(JSON_STREAM_PROTOCOL_VERSION, "0.2.0");
    assert_eq!(value["type"], "ready");
    assert_eq!(value["version"], "0.2.0");
    assert!(value.get("session_id").is_none());
}
