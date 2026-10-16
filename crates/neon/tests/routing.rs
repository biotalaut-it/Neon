use neon::agent::{route_categories, AgentRole};

#[test]
fn routes_categories_to_specialists() {
    let selected = route_categories(&["coding", "maintenance", "debugging", "decompose"]).unwrap();
    assert_eq!(
        selected,
        vec![
            AgentRole::Programmer,
            AgentRole::Maintenance,
            AgentRole::Debugger,
            AgentRole::Decomposer
        ]
    );
}

#[test]
fn invalid_or_ambiguous_category_fails_closed() {
    assert!(route_categories(&["unknown"]).is_err());
    assert!(route_categories(&[]).is_err());
}
