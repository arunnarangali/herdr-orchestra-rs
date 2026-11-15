# Security review (plan §22)

Reviewed on the Phase 10 branch, after M2 (failure fixes) and M3 (permission prompts). Each item names where it is implemented, or the gap found.

| §22 item | Where | Result |
|---|---|---|
| Secure credential handling | `orch-api/src/token.rs`: random 32-byte token in the config dir (dir 0700, file 0600, written aside and hard-linked into place); an existing file with group/other bits is tightened to 0600 on read; compared in constant time; `Debug` is redacted | OK. Tests in `token.rs` |
| API bound to loopback only | `orch-api/src/lib.rs` `bind`: any non-loopback address is `BindError::NotLoopback` | OK |
| Origin / DNS-rebinding checks | `orch-api/src/guard.rs` `origin`: non-loopback `Host` and any foreign `Origin` are refused before auth; no CORS | OK. Tests in `orch-api/tests` |
| Auth on every route | `guard.rs` `auth`: Bearer token; `?token=` only on the stream routes (event sources cannot set headers) | OK |
| No plaintext secrets in logs | Token never printed (`serve_cmd.rs` prints only the token *path*); the API has no request-logging layer, so the `?token=` query is never logged; `tracing` only records error causes. `events.jsonl` holds lifecycle names and timestamps only | OK. Recorded risk: a blocked agent's visible prompt line (M3) is stored in the lead queue and shown by the API. It is one line, control characters stripped, capped at 160 chars, but it is whatever the pane shows. The fix is not to echo secrets into panes, not code |
| No unnecessary env exposure | Launch lines carry no secrets and orch adds no environment variables to agent panes (`launch.rs` builds a command string only); the herdr CLI wrapper sets only the variables the caller passes (`orch-herdr/src/cli.rs`) | OK (not exhaustively traced through every herdr call) |
| Safe command construction | Launch lines: every extra arg, model and `allow_commands` entry is shell-quoted (`orch-config/src/launch.rs` `shell_quote`); `allow_commands` entries reject `;|&$\`` and `*` (`orch-config/tests/allow_commands.rs`). Git is always run with an argv, never a shell. Only `check.rs` and `setup.rs` pass a string to `sh -c`, and that string is the plan's own `Check command` / `Setup command`. Agent output never reaches a shell: it is only logged, stored, or typed into a pane as a prompt | OK. Guard test `orch-core/tests/safeguards.rs` fails if any other file starts a shell |
| Plan text reaching commands | Milestone ids are `M<digits>` (`plan.rs` `parse_heading`); branches pass `git check-ref-format --branch`; models are quoted | **Gap found and fixed**: agent names (`Workers:`) allowed `..`, `.`, a leading `-` or `.`. A name becomes the worktree folder (`.orchestrate/worktrees/<name>`) and a git argument, so `..` left the folder. `orch sync` now rejects them: "is not a safe agent name". Test `an_agent_name_that_could_leave_its_folder_or_pass_for_an_option_is_an_error` (fails without the fix) |
| Proper process termination | `check.rs` kills the whole process group on timeout | **Gap found and fixed**: `setup.rs` only killed `sh`, so a hung setup's children kept running. It now runs in its own process group and the group is killed. Test `a_hung_setup_is_killed_with_its_children_and_says_it_timed_out` |
| Project directory validation | `orch-core/src/paths.rs` `main_root`: the run is found through git (`rev-parse`), so a folder that is not a repo is a clear error; the API serves only the one run's root it was started with and builds paths from fixed names (`plan.md`), never from request text | OK |
| Git safeguards | Only `orch-core/src/finish.rs` runs `git push` (no force, no `--mirror`); `git stash push` in `refs.rs` is local | OK. Guard test `only_orch_finish_pushes_and_nothing_force_pushes` |
| Risk hold | `orch-core/src/merge.rs` risk rules (deletions, migrations, CI config, secrets files, dependency changes) and `scope.rs` | OK. Tests in `merge_land.rs` |
| Model deny policy | `sync.rs` (plan validation) and `launch.rs` (launch) both call `Catalog::denied` | OK |
| Config validation | agents registry (`allow_commands`, `allow_flag`), models catalog, tunables are validated on load | OK |
| Permission checks | Launch profiles never turn permissions off by default; `allow_commands` is empty unless the user sets it (M3) | OK |
| Audit logs | `events.jsonl` lifecycle lines; forced transitions are recorded in the history table (`forced` column) | OK |
| Untrusted agent/repo content | Done files are parsed as JSON data; nothing from them is executed | OK |

Not covered (out of scope): third-party penetration testing.
