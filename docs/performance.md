# Performance

## Repeatable test

`crates/orch/tests/performance.rs` builds a synthetic plan of 50 milestones with 4 simulated coders and a reviewer, runs the real manager to completion with fake agents, then starts the real API server and times `GET /api/v1/status` and `GET /api/v1/events` (100 requests each).

It is opt-in (`#[ignore]`) so the default workspace test run stays fast:

    cargo test -p orch --test performance -- --ignored --nocapture

## Numbers (debug build, one run)

| Measure | Result |
|---|---|
| Milestones merged | 50 of 50, 36 manager cycles, 0.44 s total |
| Manager cycle | avg 10.1 ms, p95 14.6 ms, max 14.7 ms |
| API `status` | avg 3.5 ms, p95 3.9 ms |
| API `events` | avg 1.3 ms, p95 1.6 ms |

## Hot spots

* None that matter: a cycle is about 10 ms with 50 milestones, and the real loop waits seconds between cycles. Most of a cycle is SQLite writes (state save, transitions).
* Added migration `0011_perf_indexes.sql`: indexes for the per-cycle `latest_delivery` lookup and the lead-queue unread lookup. At 50 milestones the effect is below the noise of this test, so no speed-up is claimed; the indexes keep those lookups from growing with run length.
* Not measured: a real herdr (each `agent status` call is a process spawn there and will dominate a real cycle) and release builds.
