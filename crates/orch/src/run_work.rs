//! The event manager's real [`Work`] (§13, §24 Phase 8): everything `orch autopilot` asks of a
//! milestone from dispatch to review — prepare and save its work (git), write its contract, run
//! the plan's check in the right folder, build the fix prompts, find the files the coder changed
//! outside its `Likely files`, and run the review round: `review_body` builds the reviewer's
//! prompt and `review_verdict` reads the strict `review-N.json` (a missing or malformed file, or a
//! reviewer that changed files, is reported as unusable). Landing is `GitWork`'s.
//!
//! The one rule of the checks: they run in the folder the code is in. A milestone's check runs in
//! its coder's worktree (worktree mode) or the main folder (shared mode); the final check and the
//! baseline run in the main folder, which must be on the run branch. A folder that is not the
//! expected one is an error, never a silent check of something else.
//!
//! The review logic itself (schema, prompt, diff, snapshot) lives in `orch_core::review`; the two
//! review methods here only wire it to the run's folders.

use crate::task_cmds::write_task;
use orch_core::check::{self, CheckRun, Records};
use orch_core::fix::{self, FixContext, FixKind};
use orch_core::git;
use orch_core::manager::{CheckOutcome, CommitOutcome, GitWork, ReviewOutcome, Work};
use orch_core::paths::ORCH_DIR;
use orch_core::refs::{self, task_ref};
use orch_core::review;
use orch_core::run_store::RunSettings;
use orch_core::scope;
use orch_core::store::Store;
use orch_core::task::Task;
use orch_core::worktree;
use std::path::{Path, PathBuf};
use std::time::Duration;

type Res<T> = std::result::Result<T, String>;

fn write(path: &Path, text: &str) -> Res<()> {
    std::fs::write(path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// The task name the manager gives the branch-wide check.
pub const FINAL_CHECK: &str = "(final check)";

pub struct RunWork {
    git: GitWork,
    root: PathBuf,
    limit: Duration,
    /// The coder `prepare` just put a milestone's folder on: `write_task` runs before the claim
    /// that records it.
    prepared: Option<(String, String)>,
}

impl RunWork {
    /// A `Work` over the run at `root`, with `ORCH_CHECK_TIMEOUT_MIN` as the check's limit.
    pub fn new(root: PathBuf) -> Self {
        Self::with_limit(root, check::timeout_from_env())
    }

    pub fn with_limit(root: PathBuf, limit: Duration) -> Self {
        RunWork { git: GitWork::new(root.clone()), root, limit, prepared: None }
    }

    fn store(&self) -> Res<Store> {
        Store::open_in(&self.root.join(ORCH_DIR)).map_err(|e| e.to_string())
    }

    fn settings(&self, store: &Store) -> Res<RunSettings> {
        store.run_settings().map_err(|e| e.to_string())?.ok_or_else(|| "no run in state.db".to_string())
    }

    /// The folder the milestone's coder works in: the coder's worktree (worktree mode; it must
    /// exist) or the main folder on the run branch (shared mode).
    fn coder_dir(&self, s: &RunSettings, t: &Task) -> Res<PathBuf> {
        if s.isolation != "worktree" {
            self.main_on_branch(s)?;
            return Ok(self.root.clone());
        }
        let coder = match &self.prepared {
            Some((task, w)) if *task == t.id => Some(w.clone()),
            _ => t.worker.clone().or_else(|| t.last_worker.clone()),
        };
        let Some(coder) = coder else {
            return Err(format!("{} has no coder folder yet", t.id));
        };
        let dir = worktree::worktree_path(&self.root, &coder);
        if !dir.is_dir() {
            return Err(format!("{}'s folder {} does not exist", t.id, dir.display()));
        }
        Ok(dir)
    }

    /// The main folder must be on the run branch before anything is checked or measured there.
    fn main_on_branch(&self, s: &RunSettings) -> Res<()> {
        let now = git::current_branch(&self.root).map_err(|e| e.to_string())?;
        if now == s.branch {
            Ok(())
        } else {
            Err(format!("the main folder is on {now}, not on the run branch {}", s.branch))
        }
    }

    fn run_check(&self, s: &RunSettings, dir: &Path) -> CheckRun {
        check::run(&s.check_command, dir, self.limit, check::TAIL_LINES)
    }

    fn outcome(run: &CheckRun) -> CheckOutcome {
        if run.ok { CheckOutcome::Pass } else { CheckOutcome::Fail { tail: run.tail.clone() } }
    }
}

impl Work for RunWork {
    fn prepare(&mut self, task: &str, worker: &str) -> Res<()> {
        self.git.prepare(task, worker)?;
        self.prepared = Some((task.to_string(), worker.to_string()));
        Ok(())
    }

    /// The contract files, in the folder the coder works in (the milestone's worktree in
    /// worktree mode, else the main folder), by the same code as `orch write-task`.
    fn write_task(&mut self, task: &str) -> Res<()> {
        let store = self.store()?;
        let s = self.settings(&store)?;
        let t = store.get_task(task).map_err(|e| e.to_string())?;
        let dir = self.coder_dir(&s, &t)?;
        write_task::generate_for(task, &self.root, Some(&dir)).map(|_| ()).map_err(|e| e.to_string())
    }

    fn fix_body(&mut self, task: &str, round: u32, tail: &str) -> Res<String> {
        let kind = if tail == "result file" {
            FixKind::ResultFile
        } else {
            FixKind::Check { tail: tail.to_string() }
        };
        self.fix_prompt(task, round, &kind)
    }

    fn fix_prompt(&mut self, task: &str, round: u32, kind: &FixKind) -> Res<String> {
        let store = self.store()?;
        let s = self.settings(&store)?;
        let t = store.get_task(task).map_err(|e| e.to_string())?;
        // a rewrite of the result file answers the attempt in progress; any other fix is the
        // next attempt
        let attempt = if *kind == FixKind::ResultFile { t.attempt.max(1) } else { t.attempt + 1 };
        let result_file = format!("result-{attempt}.json");
        Ok(fix::body(
            kind,
            &FixContext { task, title: &t.title, check: &s.check_command, round, result_file: &result_file },
        ))
    }

    /// The plan's check, in the milestone's coder folder; `(final check)` on the run branch in the
    /// main folder. Recorded for the milestone as `worktree` / `branch`, seconds and time.
    fn check(&mut self, task: &str) -> Res<CheckOutcome> {
        let store = self.store()?;
        let s = self.settings(&store)?;
        let (dir, place) = if task == FINAL_CHECK {
            self.main_on_branch(&s)?;
            (self.root.clone(), "branch")
        } else {
            let t = store.get_task(task).map_err(|e| e.to_string())?;
            (self.coder_dir(&s, &t)?, "worktree")
        };
        let run = self.run_check(&s, &dir);
        Records::record_check(&self.root, task, place, &run).map_err(|e| e.to_string())?;
        Ok(Self::outcome(&run))
    }

    fn baseline(&mut self) -> Res<(bool, String)> {
        let store = self.store()?;
        let s = self.settings(&store)?;
        self.main_on_branch(&s)?;
        let run = self.run_check(&s, &self.root);
        Ok((run.ok, run.tail))
    }

    fn strays(&mut self, task: &str) -> Res<Vec<String>> {
        let store = self.store()?;
        let s = self.settings(&store)?;
        let allowed = store
            .load_run()
            .map_err(|e| e.to_string())?
            .and_then(|r| r.plan.into_iter().find(|p| p.id == task).map(|p| p.files))
            .unwrap_or_default();
        if allowed.is_empty() {
            return Ok(Vec::new());
        }
        let t = store.get_task(task).map_err(|e| e.to_string())?;
        let dir = self.coder_dir(&s, &t)?;
        let changed =
            scope::changed_files(&dir, &s.branch, s.isolation != "worktree").map_err(|e| e.to_string())?;
        Ok(scope::strays(&changed, &allowed))
    }

    fn save_work(&mut self, task: &str) -> Res<()> {
        self.git.save_work(task)
    }

    /// The review prompt for round `round`: writes the milestone's diff against the merge-base
    /// (`diff.patch`), a copy of its contract, and the main folder's snapshot (so a reviewer that
    /// edits files is caught) into `.orchestrate/review/<Mn>/`, and returns the prompt text.
    fn review_body(&mut self, task: &str, round: u32) -> Res<String> {
        let store = self.store()?;
        let s = self.settings(&store)?;
        let t = store.get_task(task).map_err(|e| e.to_string())?;
        let orch = self.root.join(ORCH_DIR);
        let rd = review::review_dir(&orch, task);
        std::fs::create_dir_all(&rd).map_err(|e| format!("creating {}: {e}", rd.display()))?;
        let coder_dir = self.coder_dir(&s, &t).ok();

        // the saved work (both modes park it in the hidden ref before review); else the folder
        let src = refs::read(&self.root, &task_ref(&s.branch, task)).map_err(|e| e.to_string())?;
        let diff = match (&src, &coder_dir) {
            (Some(src), _) => review::diff_of_ref(&self.root, &s.branch, src)?,
            (None, Some(dir)) => review::snapshot_diff(dir, &s.branch)?,
            (None, None) => {
                return Err(format!("{task} has no saved work ({}) to review", task_ref(&s.branch, task)));
            }
        };
        write(&rd.join("diff.patch"), &diff)?;
        let issue = store.issue(task).map_err(|e| e.to_string())?;
        let resolution = match (&issue, &src) {
            (Some(i), Some(src)) if i.rereview && i.resolution_base.is_some() => {
                let base = i.resolution_base.as_deref().unwrap_or_default();
                let args = ["diff", base, src.as_str()];
                let d = git::run(&self.root, &args).map_err(|e| e.to_string())?;
                write(&rd.join("resolution.patch"), &d.stdout)?;
                true
            }
            _ => false,
        };
        for name in ["task.md", "task.json"] {
            let from = [coder_dir.as_deref(), Some(self.root.as_path())]
                .into_iter()
                .flatten()
                .map(|d| d.join(".orch-task").join(task).join(name))
                .find(|p| p.is_file());
            if let Some(from) = from {
                std::fs::copy(&from, rd.join(name)).map_err(|e| format!("copying {name}: {e}"))?;
            }
        }
        review::write_snapshot(&self.root, task, round).map_err(|e| e.to_string())?;
        // the poll watches `.orch-task/<Mn>/done-<attempt>`: the coder's own done file for this
        // attempt is still there, and would end the review before it starts
        let done = format!(".orch-task/{task}/done-{}", t.attempt.max(1));
        let _ = std::fs::remove_file(self.root.join(&done));
        Ok(review::prompt(task, &t.title, round, &done, resolution))
    }

    /// The reviewer's verdict for `round`: `Approved` (approved, or approved with warnings, whose
    /// suggestions are kept for the summary), `Blockers` (changes requested or blocked), or
    /// `Unusable` with the reason — a missing, empty, malformed or self-contradicting file, a
    /// criterion left unjudged, or a reviewer that changed files in the main folder.
    fn review_verdict(&mut self, task: &str, round: u32) -> Res<ReviewOutcome> {
        let unusable = |line: String| Ok(ReviewOutcome::Unusable { line });
        let changed = review::changed_since(&self.root, task, round).map_err(|e| e.to_string())?;
        if !changed.is_empty() {
            return unusable(format!(
                "the reviewer changed files in the repository during review round {round} ({}) — the review is not trusted",
                changed.join(", ")
            ));
        }
        let orch = self.root.join(ORCH_DIR);
        let r = match review::read_round(&orch, task, round) {
            Ok(r) => r,
            Err(line) => return unusable(line),
        };
        let wanted = review::contract_criteria(&orch, task);
        let unjudged = review::unjudged(&r, &wanted);
        if !unjudged.is_empty() {
            return unusable(format!(
                "review output not usable: review-{round}.json gives no verdict for {}",
                unjudged.join(", ")
            ));
        }
        review::store_suggestions(&orch, task, round, &r.suggestion_lines()).map_err(|e| e.to_string())?;
        Ok(if r.outcome.approves() {
            ReviewOutcome::Approved
        } else {
            ReviewOutcome::Blockers { count: r.blocker_count() }
        })
    }

    /// A queued job's file goes into the coder's folder (`prepare` has just put it there), where
    /// the prompt says to read it.
    fn write_job(&mut self, _root: &Path, task: &str, file: &str, body: &str) -> Res<()> {
        let store = self.store()?;
        let s = self.settings(&store)?;
        let t = store.get_task(task).map_err(|e| e.to_string())?;
        let dir = self.coder_dir(&s, &t)?.join(".orch-task").join(task);
        std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        write(&dir.join(file), body)
    }

    fn blocker_lines(&mut self, task: &str, round: u32) -> Res<Vec<String>> {
        let r = review::read_round(&self.root.join(ORCH_DIR), task, round)?;
        Ok(r.blocker_lines())
    }

    fn commit(&mut self, task: &str) -> Res<CommitOutcome> {
        self.git.commit(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orch_core::run_store::{AgentRole, PlanTask, RunAgent, SyncInput};
    use orch_core::store::Counter;

    const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Check command: test -e marker.txt
Workers: w1 (claude: sonnet)
Reviewer: none

## [x] M1: One
Depends on: none
Tier: medium
Model why: normal work
Likely files: src/, docs/*.md
Goal:
- g
Acceptance criteria:
- works
";

    fn g(dir: &Path, args: &[&str]) {
        let mut a = vec!["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"];
        a.extend(args);
        let o = git::run(dir, &a).unwrap();
        assert!(o.success(), "git {args:?}: {}", o.text());
    }

    struct Fx {
        _t: tempfile::TempDir,
        root: PathBuf,
        store: Store,
        work: RunWork,
    }

    impl Fx {
        fn new(isolation: &str, check: &str) -> Fx {
            let t = tempfile::tempdir().unwrap();
            let root = t.path().canonicalize().unwrap().join("repo");
            std::fs::create_dir(&root).unwrap();
            g(&root, &["init", "-q", "-b", "main"]);
            // an identity in the repo itself (its worktrees share it): saving work must not depend
            // on whatever git config the machine running the tests happens to have
            g(&root, &["config", "user.name", "t"]);
            g(&root, &["config", "user.email", "t@t"]);
            std::fs::write(root.join("a.txt"), "a\n").unwrap();
            g(&root, &["add", "-A"]);
            g(&root, &["commit", "-q", "-m", "init"]);
            let orch = root.join(ORCH_DIR);
            std::fs::create_dir_all(&orch).unwrap();
            std::fs::write(orch.join("plan.md"), PLAN).unwrap();
            let mut store = Store::open_in(&orch).unwrap();
            store
                .apply_sync(&SyncInput {
                    settings: RunSettings {
                        branch: "feature/demo".into(),
                        base: Some("main".into()),
                        title: "Demo".into(),
                        commits: "per-milestone".into(),
                        isolation: isolation.into(),
                        max_parallel: 1,
                        setup_inputs: vec![],
                        scope: "warn".into(),
                        risk_hold: false,
                        budget_s: None,
                        check_command: check.into(),
                        setup_command: "none".into(),
                        lead_mode: "off".into(),
                        reviewer: None,
                    },
                    agents: vec![RunAgent {
                        role: AgentRole::Worker,
                        name: "w1".into(),
                        kind: "claude".into(),
                        model: None,
                        tier: None,
                        detect: true,
                        pane: None,
                    }],
                    tasks: vec![PlanTask {
                        id: "M1".into(),
                        title: "One".into(),
                        deps: vec![],
                        enabled: true,
                        open_questions: false,
                        tier: Some("medium".into()),
                        needs: vec![],
                        model_why: String::new(),
                        prefer: "any".into(),
                        review: false,
                        files: vec!["src/".into(), "docs/*.md".into()],
                        parallel: true,
                    }],
                })
                .unwrap();
            worktree::start(&mut store, &root).unwrap();
            let work = RunWork::with_limit(root.clone(), Duration::from_secs(30));
            Fx { _t: t, root, store, work }
        }

        /// M1 is with `w1`, attempt 1.
        fn coding(&mut self) -> PathBuf {
            self.work.prepare("M1", "w1").unwrap();
            self.store.bump("M1", Counter::Attempt).unwrap();
            self.store.claim("M1", "w1", None).unwrap();
            worktree::worktree_path(&self.root, "w1")
        }
    }

    fn put(dir: &Path, rel: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }

    #[test]
    fn a_milestones_check_runs_in_its_coders_worktree_and_is_recorded() {
        let mut fx = Fx::new("worktree", "test -e marker.txt && echo MARKED");
        let wt = fx.coding();
        // the marker exists only in the worktree: passing proves the folder
        assert_eq!(fx.work.check("M1").unwrap(), CheckOutcome::Fail { tail: String::new() });
        put(&wt, "marker.txt");
        assert_eq!(fx.work.check("M1").unwrap(), CheckOutcome::Pass);
        let rec = Records::load(&fx.root).0["M1"].clone();
        assert_eq!(rec.checks.len(), 2);
        assert!(!rec.checks[0].ok && rec.checks[1].ok);
        assert!(rec.checks.iter().all(|c| c.place == "worktree" && c.ts > 0));
    }

    #[test]
    fn the_final_check_and_the_baseline_run_in_the_main_folder_on_the_run_branch() {
        let mut fx = Fx::new("worktree", "test -e marker.txt && echo MARKED");
        let wt = fx.coding();
        put(&wt, "marker.txt");
        // the main folder has no marker: red there even though the coder's folder is green
        assert!(matches!(fx.work.check(FINAL_CHECK).unwrap(), CheckOutcome::Fail { .. }));
        assert!(!fx.work.baseline().unwrap().0);
        put(&fx.root, "marker.txt");
        assert_eq!(fx.work.check(FINAL_CHECK).unwrap(), CheckOutcome::Pass);
        let (ok, tail) = fx.work.baseline().unwrap();
        assert!(ok && tail.contains("MARKED"), "{tail}");
        let rec = Records::load(&fx.root).0[FINAL_CHECK].clone();
        assert_eq!(rec.checks.len(), 2);
        assert!(rec.checks.iter().all(|c| c.place == "branch"));
        // the baseline is recorded nowhere
        assert_eq!(Records::load(&fx.root).0.len(), 1);
    }

    #[test]
    fn a_main_folder_on_the_wrong_branch_is_never_checked() {
        let mut fx = Fx::new("worktree", "true");
        g(&fx.root, &["switch", "-q", "-c", "elsewhere"]);
        let e = fx.work.check(FINAL_CHECK).unwrap_err();
        assert!(e.contains("elsewhere") && e.contains("feature/demo"), "{e}");
        assert!(fx.work.baseline().is_err());
        assert!(Records::load(&fx.root).0.is_empty());
    }

    #[test]
    fn a_hung_check_fails_with_the_timeout_line() {
        let mut fx = Fx::new("worktree", "sleep 30");
        fx.coding();
        fx.work = RunWork::with_limit(fx.root.clone(), Duration::from_millis(200));
        match fx.work.check("M1").unwrap() {
            CheckOutcome::Fail { tail } => assert!(tail.contains("timed out"), "{tail}"),
            CheckOutcome::Pass => panic!("a hung check must fail"),
        }
    }

    #[test]
    fn shared_mode_checks_in_the_main_folder() {
        let mut fx = Fx::new("shared", "test -e marker.txt");
        fx.work.prepare("M1", "w1").unwrap();
        fx.store.bump("M1", Counter::Attempt).unwrap();
        fx.store.claim("M1", "w1", None).unwrap();
        assert!(matches!(fx.work.check("M1").unwrap(), CheckOutcome::Fail { .. }));
        put(&fx.root, "marker.txt");
        assert_eq!(fx.work.check("M1").unwrap(), CheckOutcome::Pass);
    }

    #[test]
    fn write_task_puts_the_contract_in_the_coders_worktree_only() {
        let mut fx = Fx::new("worktree", "true");
        let wt = fx.coding();
        fx.work.write_task("M1").unwrap();
        let dir = wt.join(".orch-task/M1");
        let md = std::fs::read_to_string(dir.join("task.md")).unwrap();
        assert!(md.contains("# Task M1: One") && md.contains("## Check command"), "{md}");
        let js = std::fs::read_to_string(dir.join("task.json")).unwrap();
        assert!(js.contains("\"task_id\": \"M1\""), "{js}");
        assert!(!fx.root.join(".orch-task/M1").exists(), "not in the main folder");
    }

    #[test]
    fn write_task_before_the_claim_uses_the_coder_just_prepared() {
        let mut fx = Fx::new("worktree", "true");
        fx.work.prepare("M1", "w1").unwrap(); // dispatch order: prepare, write_task, then claim
        fx.work.write_task("M1").unwrap();
        assert!(worktree::worktree_path(&fx.root, "w1").join(".orch-task/M1/task.json").exists());
    }

    #[test]
    fn strays_use_the_real_diff_with_folder_and_glob_entries() {
        let mut fx = Fx::new("worktree", "true");
        let wt = fx.coding();
        for f in ["src/deep/a.rs", "docs/guide.md", "Cargo.toml", "other/y.txt", "docs/guide.txt"] {
            put(&wt, f);
        }
        assert_eq!(
            fx.work.strays("M1").unwrap(),
            vec!["Cargo.toml", "docs/guide.txt", "other/y.txt"],
            "src/ (folder) and docs/*.md (glob) are covered"
        );
    }

    #[test]
    fn fix_prompts_name_the_result_file_to_write_next() {
        let mut fx = Fx::new("worktree", "make check");
        fx.coding(); // attempt 1
        let b = fx.work.fix_body("M1", 1, "line one\nline two").unwrap();
        assert!(b.contains("`make check`") && b.contains("line two") && b.contains(".orch-task/M1/result-2.json"), "{b}");
        let b = fx.work.fix_body("M1", 1, "result file").unwrap();
        assert!(b.contains("not usable") && b.contains("result-1.json"), "{b}");
        let kind = FixKind::Criteria { unmet: vec![("AC3".into(), "no test".into())] };
        let b = fx.work.fix_prompt("M1", 2, &kind).unwrap();
        assert!(b.contains("- AC3: no test") && b.contains("round 2"), "{b}");
        let kind = FixKind::Blockers { round: 1, lines: vec!["BLOCKER: a.rs:1 — x — y".into()] };
        assert!(fx.work.fix_prompt("M1", 1, &kind).unwrap().contains("BLOCKER: a.rs:1 — x — y"));
    }

    /// M1's work saved to its hidden ref, as the manager does before review; contract written.
    fn reviewable(fx: &mut Fx) {
        let wt = fx.coding();
        fx.work.write_task("M1").unwrap();
        put(&wt, "src/new.rs");
        fx.work.save_work("M1").unwrap();
    }

    fn write_review(fx: &Fx, round: u32, text: &str) {
        let d = fx.root.join(ORCH_DIR).join("review/M1");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(format!("review-{round}.json")), text).unwrap();
    }

    const OK: &str = r#"{"outcome":"approved","criteria":[{"id":"AC1","met":true,"note":"ok"}],"blockers":[],"suggestions":[]}"#;

    #[test]
    fn the_review_prompt_has_the_diff_the_contract_and_the_rules() {
        let mut fx = Fx::new("worktree", "true");
        reviewable(&mut fx);
        let body = fx.work.review_body("M1", 1).unwrap();
        let rd = fx.root.join(ORCH_DIR).join("review/M1");
        let diff = std::fs::read_to_string(rd.join("diff.patch")).unwrap();
        assert!(diff.contains("src/new.rs") && !diff.contains("a.txt"), "{diff}");
        assert!(rd.join("task.md").is_file() && rd.join("task.json").is_file());
        for want in [
            ".orchestrate/review/M1/review-1.json", "BLOCKER: file:line — what — why", "Do NOT modify",
            ".orchestrate/review/M1/diff.patch", ".orch-task/M1/done-1",
        ] {
            assert!(body.contains(want), "{want}\n{body}");
        }
    }

    #[test]
    fn a_stale_coder_done_file_is_removed_so_the_review_waits_for_the_reviewer() {
        let mut fx = Fx::new("worktree", "true");
        reviewable(&mut fx);
        let done = fx.root.join(".orch-task/M1/done-1");
        std::fs::create_dir_all(done.parent().unwrap()).unwrap();
        std::fs::write(&done, "").unwrap();
        fx.work.review_body("M1", 1).unwrap();
        assert!(!done.exists());
    }

    #[test]
    fn a_queued_job_is_written_into_the_coders_folder() {
        let mut fx = Fx::new("worktree", "true");
        let wt = fx.coding();
        let root = fx.root.clone();
        fx.work.write_job(&root, "M1", "review-fix-1.md", "# job").unwrap();
        assert_eq!(std::fs::read_to_string(wt.join(".orch-task/M1/review-fix-1.md")).unwrap(), "# job");
        assert!(!root.join(".orch-task/M1/review-fix-1.md").exists());
    }

    #[test]
    fn verdicts_map_to_the_manager_outcomes() {
        let mut fx = Fx::new("worktree", "true");
        reviewable(&mut fx);
        fx.work.review_body("M1", 1).unwrap();
        write_review(&fx, 1, OK);
        assert_eq!(fx.work.review_verdict("M1", 1).unwrap(), ReviewOutcome::Approved);
        write_review(
            &fx,
            1,
            r#"{"outcome":"approved-with-warnings","criteria":[{"id":"AC1","met":true}],"blockers":[],
                "suggestions":[{"file":"a.rs","line":1,"problem":"p","why":"w"}]}"#,
        );
        assert_eq!(fx.work.review_verdict("M1", 1).unwrap(), ReviewOutcome::Approved);
        // the warning is kept on the milestone for the summary
        let kept = review::suggestions_section(&fx.root.join(ORCH_DIR), &["M1".to_string()]);
        assert_eq!(kept, vec!["- M1: SUGGESTION: a.rs:1 — p — w"]);
        write_review(
            &fx,
            1,
            r#"{"outcome":"changes-requested","criteria":[{"id":"AC1","met":false,"note":"n"}],
                "blockers":[{"file":"a.rs","line":2,"problem":"p","why":"w"}],"suggestions":[]}"#,
        );
        assert_eq!(fx.work.review_verdict("M1", 1).unwrap(), ReviewOutcome::Blockers { count: 2 });
        assert_eq!(fx.work.blocker_lines("M1", 1).unwrap().len(), 2);
        write_review(
            &fx,
            1,
            r#"{"outcome":"blocked","criteria":[{"id":"AC1","met":true}],
                "blockers":[{"file":"a.rs","problem":"p"}],"suggestions":[]}"#,
        );
        assert_eq!(fx.work.review_verdict("M1", 1).unwrap(), ReviewOutcome::Blockers { count: 1 });
    }

    #[test]
    fn missing_empty_malformed_and_incomplete_files_are_unusable_with_a_reason() {
        let mut fx = Fx::new("worktree", "true");
        reviewable(&mut fx);
        fx.work.review_body("M1", 1).unwrap();
        let line = |fx: &mut Fx| match fx.work.review_verdict("M1", 1).unwrap() {
            ReviewOutcome::Unusable { line } => line,
            other => panic!("expected Unusable, got {other:?}"),
        };
        assert!(line(&mut fx).contains("review-1.json is missing"));
        write_review(&fx, 1, "");
        assert!(line(&mut fx).contains("is empty"));
        write_review(&fx, 1, "{oops");
        assert!(line(&mut fx).contains("not valid JSON"));
        write_review(&fx, 1, r#"{"outcome":"maybe","criteria":[],"blockers":[],"suggestions":[]}"#);
        assert!(line(&mut fx).contains("`outcome`"));
        // approved, but the contract's criterion AC1 was never judged
        write_review(&fx, 1, r#"{"outcome":"approved","criteria":[],"blockers":[],"suggestions":[]}"#);
        assert!(line(&mut fx).contains("no verdict for AC1"));
    }

    #[test]
    fn a_reviewer_that_edited_the_repository_makes_the_review_unusable() {
        let mut fx = Fx::new("worktree", "true");
        reviewable(&mut fx);
        fx.work.review_body("M1", 1).unwrap();
        write_review(&fx, 1, OK);
        put(&fx.root, "sneaky.txt"); // the reviewer's edit in the main folder
        match fx.work.review_verdict("M1", 1).unwrap() {
            ReviewOutcome::Unusable { line } => {
                assert!(line.contains("changed files") && line.contains("sneaky.txt"), "{line}");
            }
            other => panic!("expected Unusable, got {other:?}"),
        }
    }
}
