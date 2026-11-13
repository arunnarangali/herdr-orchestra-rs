# Task M1: Foundation

Title: Foundation
Depends on: none
Why separate: the base others build on
Prefer: any
Tier: small
Needs: coding
Model why: mechanical work on a known pattern
Review: no
Likely files: src/a.rs, `src/b/`
Goal:
- Do the first thing well.
- Keep it small.
Why: it is the base the rest builds on.
Acceptance criteria:
- the first thing works
  - in every case
- the second thing is covered
Constraints:
- stay in src/
Interfaces:
- none
Context:
- the code lives in src/
Out of scope:
- nothing else
Open questions:
- none
Notes from user:

## Global instructions
- Be tidy.
  - Indented note.
- Never break the build.

## Acceptance criteria ids (for your result file)
- AC1: the first thing works
- AC2: in every case
- AC3: the second thing is covered

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
