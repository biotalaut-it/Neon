use neon::{
    agent::{route_categories, router::TaskIntent, AgentRole},
    tools::{neonatom_policy, should_use_neonatom, NeonAtomPolicy},
};

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
fn compute_tasks_are_recognized_separately_from_agent_routing() {
    assert_eq!(
        TaskIntent::classify("Jumlah kuadrat bilangan 1 sampai 10.").unwrap(),
        TaskIntent::Compute
    );
    assert_eq!(
        TaskIntent::classify("Tolong perbaiki bug di Rust ini.").unwrap(),
        TaskIntent::Debugging
    );
    assert_eq!(
        TaskIntent::classify("Tulis fungsi Rust untuk parsing JSON.").unwrap(),
        TaskIntent::Coding
    );
    assert_eq!(
        TaskIntent::classify("Jelaskan apa itu ownership di Rust.").unwrap(),
        TaskIntent::General
    );
}

#[test]
fn neonatom_policy_only_allows_small_clear_compute_tasks() {
    assert!(should_use_neonatom("sum(map(range(1, 11), x => x * x))").unwrap());
    assert!(should_use_neonatom("Jumlah kuadrat bilangan 1 sampai 10, termasuk 10.").unwrap());
    assert!(!should_use_neonatom("Jelaskan apa itu ownership di Rust.").unwrap());
    assert!(!should_use_neonatom("Tolong debug error Rust ini.").unwrap());
    assert!(should_use_neonatom("sampai 10").is_err());
    assert_eq!(
        neonatom_policy("Jumlah kuadrat bilangan 1 sampai 10.").unwrap(),
        NeonAtomPolicy::NeedsClarification(
            "Batas rentang belum jelas: apakah angka akhirnya ikut dihitung?".into()
        )
    );
}

#[test]
fn invalid_or_ambiguous_category_fails_closed() {
    assert!(route_categories(&["unknown"]).is_err());
    assert!(route_categories(&[]).is_err());
}
