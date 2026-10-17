use neon::{
    agent::{
        route_categories,
        router::{
            AdaptiveReasoning, HybridIntelligenceRouter, ModelRoute, ReasoningMode, TaskIntent,
        },
        AgentRole,
    },
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
fn adaptive_reasoning_uses_direct_path_only_for_simple_general_requests() {
    assert_eq!(
        AdaptiveReasoning::choose("Apa arti ownership?"),
        ReasoningMode::Direct
    );
    assert_eq!(
        AdaptiveReasoning::choose("Tolong perbaiki bug parser Rust."),
        ReasoningMode::Analyze
    );
    assert_eq!(
        AdaptiveReasoning::choose("Cari fungsi parser di repo."),
        ReasoningMode::Search
    );
    assert_eq!(
        AdaptiveReasoning::choose("Tolong test perubahan parser."),
        ReasoningMode::Test
    );
}

#[test]
fn hybrid_router_requires_explicit_consent_and_only_offloads_analysis() {
    assert_eq!(
        HybridIntelligenceRouter::choose("Perbaiki bug pada parser Rust.", false),
        ModelRoute::Local
    );
    assert_eq!(
        HybridIntelligenceRouter::choose("Perbaiki bug pada parser Rust.", true),
        ModelRoute::Remote
    );
    assert_eq!(
        HybridIntelligenceRouter::choose("Apa arti ownership?", true),
        ModelRoute::Local
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
