use neon::agent::{AgentPacket, AgentRole, PacketStatus};

#[test]
fn agent_packet_round_trips_as_compact_json() {
    let packet = AgentPacket {
        role: AgentRole::Programmer,
        task: "fix parser".into(),
        context: vec!["error at byte 4".into()],
        constraints: vec!["keep public API".into()],
        result: Some("checked addition".into()),
        evidence: vec!["test passes".into()],
        status: PacketStatus::Complete,
        confidence: Some(0.9),
    };
    let encoded = packet.encode().unwrap();
    assert!(encoded.len() < 512);
    assert_eq!(AgentPacket::decode(&encoded).unwrap(), packet);
}

#[test]
fn invalid_role_labels_are_rejected() {
    assert_eq!(
        AgentRole::parse_label("programmer").unwrap(),
        AgentRole::Programmer
    );
    assert!(AgentRole::parse_label("root_shell").is_err());
}
