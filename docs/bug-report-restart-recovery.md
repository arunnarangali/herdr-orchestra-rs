# Bug report: after a machine or herdr restart, a run does not recover and wrongly pauses work

**Affects:** the Python event manager, `orch` v4.10.1 (the PATH wrapper reports 5.0.0).
**Found in:** the Phase 6 run on `feature/event-manager-scheduler`, 2026-11-14.

## What happened
1. A run was in progress: M3 had just merged (11:42), and opencode-1 was coding M4.
2. The user closed all herdr panes and shut the machine down. About 2 hours later they powered it on and started herdr.
3. The panes came back, but the agents did not:
   - The agents' conversations were gone.
   - The pane names (`lead`, `claude-1`, `opencode-1`, `reviewer`) were gone, so `herdr agent read opencode-1` returned `agent_not_found`.
   - The event manager was not running. Its last log line is 11:42, and `driver.lock` still held the old pid.
4. The user resumed each agent's conversation by hand and typed "opencode continue".
5. `orch autopilot` was restarted. It immediately paused M4 with `opencode-1 over its time budget (121 min)` and woke the lead.
6. `orch dispatch M4` failed with `orch prepare M4 opencode-1 -> exit 2: ERROR: ... has uncommitted changes — park or clean that worker first`, and M4 paused again.
7. The cause: opencode-1 kept working after M4 was parked and left new uncommitted files. `orch park` did not save them.
8. Those files had to be committed by hand (a WIP commit) and the hidden ref `refs/orch/<branch>/M4` moved to it before M4 could continue. Nothing was lost, but the manual steps were needed.

## Defects

**1. Time while orch was down counts as agent time (false "over budget").**
- On restart the manager measured the budget from the original start time, so the 2 hours of downtime counted.
- Evidence: `[13:31:14] M4 paused: opencode-1 over its time budget (121 min)`, one second after `[13:31:13] autopilot started`.
- Expected: time the manager was not running does not count, as with the lead wait time. Measure from the last heartbeat or the restart.

**2. Nothing restarts or reports after a reboot.**
- The manager was dead and `driver.lock` was stale, and nothing said so until the user asked.
- Expected: `status` / `events` show prominently that the event manager is not running and the run is stuck, and one resume command restarts the manager and reconciles state.

**3. Pane names are lost when herdr restarts, so agents cannot be found.**
- `herdr=NOT FOUND` / `agent_not_found` appears even though the panes exist.
- Expected: on start, re-match panes to workers by cwd (the worktree folder) and kind, and re-apply the names. Otherwise print the exact `herdr agent rename` commands.

**4. A parked task's coder keeps working, and its uncommitted work blocks the restart.**
- After `M4 parked — over its time budget`, opencode-1 was still active (the user had typed "continue"). It left uncommitted files in its worktree.
- `prepare` then refused with "uncommitted changes — park or clean that worker first".
- A later `orch park M4 pending` printed `PARKED` but saved nothing, because the task was already pending, and the worktree stayed dirty.
- Expected: `park` always snapshots uncommitted files to the hidden ref, or `prepare` snapshots them itself and continues, never refusing in a way that forces a manual choice.

**5. Resuming after a crash needs manual git work.**
- Expected: a reconcile step on start that finds a worker with unsaved work and saves it.

## How to reproduce
1. Start a run with a coder mid-milestone.
2. Close herdr and power off for a while (or kill the manager and herdr).
3. Start herdr again and resume the agents by hand.
4. Run `orch autopilot` and watch it pause the milestone as over budget.
5. Run `orch dispatch Mn` and watch it refuse because of uncommitted changes.

## Suggested fixes
- Subtract downtime from the budget (defect 1).
- Add `orch resume`, or have `status` / `events` warn when the manager is dead (defect 2).
- Re-match panes to workers by cwd and kind on start (defect 3).
- Make `park` and `prepare` snapshot uncommitted work (defects 4 and 5).

## Rust edition
These are in the Phase 6 and 7 scope (recovery reconciliation, wake-up confirmation, worktree and park handling). The Phase 6 manager was tested only with the fake runtime; defects 1 to 3 need a live check once it drives a real run.
