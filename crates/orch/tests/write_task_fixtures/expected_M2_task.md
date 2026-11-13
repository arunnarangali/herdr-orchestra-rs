# Task M2: Second

Title: Second
Depends on: M1 (needs the types)
Why separate: it can run only once the base has landed
Prefer: any
Tier: medium
Needs: coding, tool-use
Model why: normal coding with tool use
Review: yes
Likely files: src/c.rs
Goal:
- Do the second thing.
Why: it completes the feature.
Acceptance criteria:
- the second thing works
Constraints:
- stay in src/
Interfaces:
- Uses M1's types unchanged.
Context:
- the types are in src/
Out of scope:
- nothing
Open questions:
- none
Notes from user:

## Global instructions
- Be tidy.
  - Indented note.
- Never break the build.

## Context: what the tasks you depend on built
### M1: Foundation (already merged into your branch)
# M1 report
Files: src/a.rs
Did: the first thing.
Check: pass.
COMMIT: feat: a

## Acceptance criteria ids (for your result file)
- AC1: the second thing works

## Earlier work
An earlier attempt (claude-1) left work in this folder. Look at it first and continue from it unless it is wrong.

## Check command
`cargo test`

## Rules for you (the coder)
- You own the HOW: the approach, the design inside this milestone, the code, the tests and every fix. This file says what and why; it deliberately doesn't say how.
- Follow the repo's existing patterns (AGENTS.md / CLAUDE.md) and any Interfaces listed above exactly; other milestones rely on them.
- Change only what this task needs. No unrelated refactors.
- Do NOT commit, push, or switch branches. orch commits your work for you.
- Stay inside the files listed under Likely files (`files_allowed` in task.json) unless the task can't be done otherwise; say why in your report if you leave them. orch compares your diff with that list.
- Finish with a result file (the prompt names it) in the `result_format` shape from task.json: `status` done / question / blocked, `commit`, `files_changed`, the `check` you ran, one entry per acceptance criterion (`id`, `met`, `evidence`), and `usage` if your tool shows tokens or cost. orch reads it, so keep it valid JSON.
- The commit message (`commit` in the result, and the last report line `COMMIT: <commit message>`) says what changed, in the repo's commit style (for example `feat(api): add categories endpoint`). It becomes the commit message, so describe the change, not the milestone.
- If anything is ambiguous, do not guess: set status `question`, put the question in `questions` and on a report line starting `QUESTION:`, finish the files and the done file, and stop.
- Before finishing, run the check command below and record the result (pass/fail + the failing lines).
