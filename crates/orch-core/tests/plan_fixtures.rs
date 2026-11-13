//! Every archived plan (Phases 2–4) parses; key fields are checked against the files.

use orch_core::graph::DepGraph;
use orch_core::plan::{Plan, parse_plan};
use std::path::Path;

fn load(name: &str) -> Plan {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/plans")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    parse_plan(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// (id, enabled, tier, dependency ids, acceptance-criteria count)
type Row = (
    &'static str,
    bool,
    &'static str,
    &'static [&'static str],
    usize,
);

fn check(plan: &Plan, rows: &[Row]) {
    assert_eq!(plan.milestones.len(), rows.len());
    for (m, (id, enabled, tier, deps, acs)) in plan.milestones.iter().zip(rows) {
        assert_eq!(m.id, *id);
        assert_eq!(m.enabled, *enabled, "{id}");
        assert_eq!(m.tier.as_deref(), Some(*tier), "{id}");
        let got: Vec<&str> = m.depends_on.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(got, *deps, "{id}");
        assert!(m.depends_on.iter().all(|d| d.reason.is_some()), "{id}");
        assert_eq!(m.acceptance_criteria.len(), *acs, "{id}");
        assert!(!m.goal.is_empty(), "{id}");
        assert!(!m.why_separate.as_deref().unwrap_or("").is_empty(), "{id}");
        assert!(!m.has_open_questions(), "{id}");
    }
    let g = DepGraph::new(plan);
    assert!(g.issues().is_empty());
    assert!(
        plan.header
            .branch
            .as_deref()
            .is_some_and(|b| b.starts_with("feature/"))
    );
    assert!(!plan.global_instructions.is_empty());
    assert!(!plan.feature_breakdown.is_empty());
    assert!(plan.title.is_some());
}

#[test]
fn phase2_parses() {
    let p = load("phase2-rust-foundation.md");
    check(
        &p,
        &[
            ("M1", true, "medium", &[], 8),
            ("M2", true, "high", &["M1"], 10),
            ("M3", true, "medium", &["M1"], 6),
            ("M4", true, "medium", &["M1"], 2),
            ("M5", true, "high", &["M2"], 2),
            ("M6", true, "medium", &["M3"], 2),
        ],
    );
    let g = DepGraph::new(&p);
    assert!(g.independent("M2", "M3"));
    assert!(g.is_ancestor("M1", "M5"));
}

#[test]
fn phase3_parses() {
    let p = load("phase3-herdr-integration.md");
    check(
        &p,
        &[
            ("M1", true, "high", &[], 6),
            ("M2", true, "medium", &["M1"], 8),
            ("M3", true, "high", &["M1"], 7),
            ("M4", true, "medium", &["M2"], 2),
            ("M5", true, "high", &["M3"], 2),
        ],
    );
}

#[test]
fn phase4_parses_with_its_team() {
    let p = load("phase4-agent-model-management.md");
    check(
        &p,
        &[
            ("M1", true, "high", &[], 9),
            ("M2", true, "medium", &["M1"], 7),
            ("M3", true, "medium", &["M1"], 4),
            ("M4", true, "medium", &["M2"], 2),
        ],
    );
    let h = &p.header;
    assert_eq!(h.isolation.as_deref(), Some("worktree"));
    assert_eq!(h.max_parallel.as_deref(), Some("2"));
    assert_eq!(h.scope.as_deref(), Some("warn"));
    assert_eq!(h.lead[0].name, "lead");
    assert_eq!(h.lead[0].model.as_deref(), Some("opus"));
    assert_eq!(h.workers.len(), 2);
    assert_eq!(h.workers[1].kind, "opencode");
    assert_eq!(
        h.workers[1].model.as_deref(),
        Some("opencode/glm-5.3-flash-free")
    );
    assert_eq!(h.reviewer[0].kind, "agy");
    // Nested bullets stay inside their item.
    assert!(p.milestones[0].acceptance_criteria[3].contains("\n  - it returns"));
}
