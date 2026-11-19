# Prototype logic fixes (trimmed)

This is a prototype, so only issues that visibly break the demo story are kept. Everything else found in the review is deliberately dropped (see the end).

## Fix list

### 1. Keep store state across pages (verified bug)
Each screen is its own HTML document and the store re-seeds on load, so every action is lost when you change page. For example, answering the M2 question on 01 leaves it open again on 03.
- Save the store to `localStorage["herdr.state.v1"]` after each change and load it on startup. If the version doesn't match, use the seed.
- Add `actions.resetDemo()` plus a "Reset demo data" palette command.
- Done when: an action done on 01 is still applied on 02, 03, 04 and 09.

### 2. Make delayed replies go to the right project (verified bug)
`requestPlanChanges` and `submitFeatureRequest` write their lead reply from a timer using whichever project is active when the timer fires.
- Capture the project id when scheduling and pass it to `pushChat` and `pushEvent`.
- Done when: switching project during the delay leaves the reply in the original project.

### 3. Feature request updates every screen (verified bug)
It rewrites `plan` and `tasks` but not `workflow` or `run`, so 02, 03 and 04 keep the old plan. It also dismisses open attention items without warning.
- Rebuild `workflow` and `run` from the new plan in the same action.
- Mark open items "superseded" instead of dismissing them.

### 4. Start Run only reports success when it starts (verified bug)
`startRun()` returns `false` when the run is already live, but the 02 handler still shows "started" and navigates.
- Check the return value, and show "Run is already live" with no navigation when it's `false`.

### 5. Numbers agree across pages (optional)
03 shows 2/4 merged, 02 shows 1, and Overview shows 1 merged, 2 coding, 1 held.
- Cheapest fix: derive the status bar, the Overview counts and the 03 progress text from `workflow.milestones` instead of storing separate numbers. This changes some values on 01 and 04 slightly from their mockups.
- Skip this if matching each mockup's own numbers matters more than agreement between pages.

## Order
1 first, since the others are easier to test once state persists. Then 2, 3 and 4 together, then 5 if wanted.

## Dropped as not worth it for a prototype
- Making every action work on non-active projects, and removing the Attention Center's switch-and-restore.
- A run simulator and a "Demo speed" setting; the run stays static.
- Timestamp-based elapsed time.
- Settings Save logging a no-change event.
- Page 05 projects feeding the top-bar tabs.
- Event naming, extra API endpoints and the unused timer set.

## Status
Fixes 1 to 4 are implemented (`js/demo-store.js`, `js/shell.js`, `js/pages/workflows-plan.js`) and passed jsdom scenario tests. Fix 5 (matching numbers across pages) was not requested and is not done.
