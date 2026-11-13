# Plan: Sample Feature
Branch: feature/sample
Check command: cargo test
Setup command: none
Isolation: shared
Max parallel: 1
Scope: warn
Lead: lead (claude: opus)
Workers: claude-1 (claude: sonnet)
Reviewer: none

## Global instructions
- Be tidy.
  - Indented note.
- Never break the build.

## Feature breakdown
- one piece

## [x] M1: Foundation
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

## [x] M2: Second
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
