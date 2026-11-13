//! The dependency graph over a plan's milestones: the checks Python's `cmd_sync` runs
//! (unknown dependencies, self-dependencies, cycles) and the ancestor test that decides
//! which milestones are independent. Disabled milestones are part of the graph, as in Python.

use crate::plan::Plan;
use std::collections::{HashMap, HashSet};

/// One problem found in the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphIssue {
    /// `task` depends on `dep`, which is not a milestone of the plan.
    UnknownDependency { task: String, dep: String },
    /// `task` depends on itself.
    SelfDependency { task: String },
    /// A cycle; `path` starts and ends with the same id (`M1 -> M2 -> M1`).
    Cycle { path: Vec<String> },
}

impl std::fmt::Display for GraphIssue {
    /// Python's wording.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphIssue::UnknownDependency { task, dep } => {
                write!(f, "{task} depends on unknown task {dep}")
            }
            GraphIssue::SelfDependency { task } => write!(f, "{task} depends on itself"),
            GraphIssue::Cycle { path } => write!(f, "dependency cycle: {}", path.join(" -> ")),
        }
    }
}

/// Milestone ids and their `Depends on` edges.
#[derive(Debug, Clone, Default)]
pub struct DepGraph {
    /// Milestone ids in plan order, each once (a duplicated id keeps its first position).
    ids: Vec<String>,
    /// Dependencies by id; for a duplicated id the last milestone wins, as Python's dict.
    deps: HashMap<String, Vec<String>>,
    /// Every milestone's dependencies in plan order, duplicates included (for the
    /// unknown / self reports).
    edges: Vec<(String, Vec<String>)>,
}

impl DepGraph {
    pub fn new(plan: &Plan) -> Self {
        let mut g = DepGraph::default();
        for m in &plan.milestones {
            let ds: Vec<String> = m.depends_on.iter().map(|d| d.id.clone()).collect();
            if !g.deps.contains_key(&m.id) {
                g.ids.push(m.id.clone());
            }
            g.deps.insert(m.id.clone(), ds.clone());
            g.edges.push((m.id.clone(), ds));
        }
        g
    }

    /// Milestone ids in plan order, each once.
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    pub fn contains(&self, id: &str) -> bool {
        self.deps.contains_key(id)
    }

    /// Ids that more than one milestone uses, in plan order.
    pub fn duplicate_ids(&self, plan: &Plan) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut dupes = Vec::new();
        for m in &plan.milestones {
            if !seen.insert(m.id.as_str()) && !dupes.contains(&m.id) {
                dupes.push(m.id.clone());
            }
        }
        dupes
    }

    /// The direct dependencies of `id` (empty when unknown).
    pub fn dependencies(&self, id: &str) -> &[String] {
        self.deps.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Unknown and self dependencies first (in plan order), then cycles, as Python reports
    /// them. A self-dependency is also a one-edge cycle (`M1 -> M1`), as in Python.
    pub fn issues(&self) -> Vec<GraphIssue> {
        let mut out = Vec::new();
        for (task, ds) in &self.edges {
            for d in ds {
                if !self.contains(d) {
                    out.push(GraphIssue::UnknownDependency {
                        task: task.clone(),
                        dep: d.clone(),
                    });
                }
                if d == task {
                    out.push(GraphIssue::SelfDependency { task: task.clone() });
                }
            }
        }
        out.extend(
            self.cycles()
                .into_iter()
                .map(|path| GraphIssue::Cycle { path }),
        );
        out
    }

    /// Every cycle the depth-first walk meets, each as the path that closes it.
    pub fn cycles(&self) -> Vec<Vec<String>> {
        let mut color: HashMap<&str, u8> = HashMap::new();
        let mut out = Vec::new();
        for n in &self.ids {
            if !color.contains_key(n.as_str()) {
                self.dfs(n, &mut Vec::new(), &mut color, &mut out);
            }
        }
        out
    }

    fn dfs<'a>(
        &'a self,
        n: &'a str,
        stack: &mut Vec<&'a str>,
        color: &mut HashMap<&'a str, u8>,
        out: &mut Vec<Vec<String>>,
    ) {
        color.insert(n, 1);
        stack.push(n);
        for d in self.dependencies(n) {
            if !self.contains(d) {
                continue;
            }
            match color.get(d.as_str()) {
                Some(1) => {
                    let mut path: Vec<String> = stack.iter().map(|s| s.to_string()).collect();
                    path.push(d.clone());
                    out.push(path);
                }
                None => self.dfs(d, stack, color, out),
                _ => {}
            }
        }
        stack.pop();
        color.insert(n, 2);
    }

    /// Everything `id` depends on, directly or not (unknown ids are skipped). Cycle-safe.
    pub fn ancestors(&self, id: &str) -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut todo = vec![id.to_string()];
        while let Some(n) = todo.pop() {
            for d in self.dependencies(&n) {
                if self.contains(d) && seen.insert(d.clone()) {
                    todo.push(d.clone());
                }
            }
        }
        seen
    }

    /// Is `a` an ancestor of `b` (does `b` depend on `a`, directly or not)?
    pub fn is_ancestor(&self, a: &str, b: &str) -> bool {
        self.ancestors(b).contains(a)
    }

    /// Neither is an ancestor of the other: the pair `sync` checks for parallel overlap.
    pub fn independent(&self, a: &str, b: &str) -> bool {
        !self.is_ancestor(a, b) && !self.is_ancestor(b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::parse_plan;

    fn graph(deps: &[(&str, &str)]) -> (Plan, DepGraph) {
        let mut text = String::from("Branch: b\n");
        for (id, d) in deps {
            text.push_str(&format!("## [x] {id}: t\nDepends on: {d}\n"));
        }
        let p = parse_plan(&text).unwrap();
        let g = DepGraph::new(&p);
        (p, g)
    }

    #[test]
    fn clean_graph_has_no_issues() {
        let (_, g) = graph(&[("M1", "none"), ("M2", "M1 (x)"), ("M3", "M1 (x), M2 (y)")]);
        assert!(g.issues().is_empty());
        assert_eq!(g.ids(), ["M1", "M2", "M3"]);
        assert_eq!(g.dependencies("M3"), ["M1", "M2"]);
    }

    #[test]
    fn unknown_dependency() {
        let (_, g) = graph(&[("M1", "M7 (gone)")]);
        let issues = g.issues();
        assert_eq!(
            issues,
            vec![GraphIssue::UnknownDependency {
                task: "M1".into(),
                dep: "M7".into()
            }]
        );
        assert_eq!(issues[0].to_string(), "M1 depends on unknown task M7");
    }

    #[test]
    fn self_dependency() {
        let (_, g) = graph(&[("M1", "M1 (me)")]);
        let issues = g.issues();
        assert_eq!(issues[0], GraphIssue::SelfDependency { task: "M1".into() });
        assert_eq!(issues[0].to_string(), "M1 depends on itself");
        // As in Python, it is also reported as the cycle M1 -> M1.
        assert_eq!(
            issues[1],
            GraphIssue::Cycle {
                path: vec!["M1".into(), "M1".into()]
            }
        );
    }

    #[test]
    fn cycle_names_its_path() {
        let (_, g) = graph(&[
            ("M1", "M3 (a)"),
            ("M2", "M1 (b)"),
            ("M3", "M2 (c)"),
            ("M4", "none"),
        ]);
        let cycles = g.cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], ["M1", "M3", "M2", "M1"]);
        assert_eq!(
            g.issues()[0].to_string(),
            "dependency cycle: M1 -> M3 -> M2 -> M1"
        );
    }

    #[test]
    fn ancestors_and_independence() {
        let (_, g) = graph(&[
            ("M1", "none"),
            ("M2", "M1 (x)"),
            ("M3", "M1 (x)"),
            ("M4", "M2 (x)"),
            ("M5", "none"),
        ]);
        assert!(g.is_ancestor("M1", "M4"));
        assert!(!g.is_ancestor("M4", "M1"));
        assert!(!g.is_ancestor("M1", "M1"));
        assert_eq!(g.ancestors("M4"), HashSet::from(["M1".into(), "M2".into()]));
        assert!(g.independent("M2", "M3"));
        assert!(g.independent("M4", "M3"));
        assert!(g.independent("M5", "M4"));
        assert!(!g.independent("M1", "M4"));
        assert!(!g.independent("M4", "M1"));
    }

    #[test]
    fn ancestors_terminate_on_cycles_and_skip_unknown() {
        let (_, g) = graph(&[("M1", "M2 (a), M9 (b)"), ("M2", "M1 (c)")]);
        assert_eq!(g.ancestors("M1"), HashSet::from(["M1".into(), "M2".into()]));
        assert!(!g.ancestors("M1").contains("M9"));
    }

    #[test]
    fn duplicate_ids() {
        let (p, g) = graph(&[("M1", "none"), ("M2", "none"), ("M1", "M2 (x)")]);
        assert_eq!(g.duplicate_ids(&p), ["M1"]);
        assert_eq!(g.ids(), ["M1", "M2"]);
        assert_eq!(g.dependencies("M1"), ["M2"]); // last wins
    }
}
