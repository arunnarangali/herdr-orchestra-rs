---
name: orch-run
description: Orchestrator step 2 for a Herdr multi-agent team. Use when the user says "orch-run", "go", "start the run", "continue the plan", or answers a question; and whenever you are told to run `orch events`. The lead dispatches milestones and pauses; the event manager (`orch autopilot`) runs them and wakes the lead for questions, failures, held or ready milestones, and the final summary.
---

Run `orch lead run` (or `~/.local/bin/orch lead run` if `orch` is not on PATH) and follow its output exactly. You are the lead. The user's message is extra input: a go-ahead, answers, overrides, or nothing. If you were woken with `orch events`, run it and follow section 2 of the playbook.
