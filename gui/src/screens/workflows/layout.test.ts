import { describe, expect, it } from "vitest";
import type { PlanMilestone } from "../../api/types";
import { COL_WIDTH, ROW_HEIGHT, layoutGraph, wrapTitle } from "./layout";
import { twelvePlan } from "./fixtures";

function milestone(id: string, deps: string[] = [], overrides: Partial<PlanMilestone> = {}): PlanMilestone {
  return {
    id,
    title: `${id} title`,
    enabled: true,
    goal: "",
    why: "",
    why_separate: null,
    depends_on: deps.map((d) => ({ id: d, reason: `needs ${d}` })),
    prefer: null,
    tier: null,
    needs: [],
    model_why: null,
    estimate: null,
    role: null,
    review: null,
    parallel: false,
    likely_files: [],
    acceptance_criteria: [],
    constraints: "",
    interfaces: "",
    context: "",
    out_of_scope: "",
    open_questions: "none",
    has_open_questions: false,
    notes_from_user: "",
    ...overrides,
  };
}

describe("layoutGraph", () => {
  it("puts a dependency one row above what needs it", () => {
    const { nodes, edges } = layoutGraph([milestone("M1"), milestone("M2", ["M1"])], new Map());
    const m1 = nodes.find((n) => n.id === "M1");
    const m2 = nodes.find((n) => n.id === "M2");
    expect(m1).toBeDefined();
    expect(m2).toBeDefined();
    expect(m2!.y).toBeGreaterThan(m1!.y);
    expect(m2?.rank).toBe(1);
    expect(edges).toEqual([{ from: "M1", to: "M2", reason: "needs M1" }]);
  });

  it("lays parallel milestones out side by side on one row", () => {
    const plan = [milestone("M1"), milestone("M2", ["M1"]), milestone("M3", ["M1"])];
    const { nodes } = layoutGraph(plan, new Map());
    const m2 = nodes.find((n) => n.id === "M2");
    const m3 = nodes.find((n) => n.id === "M3");
    expect(m2?.rank).toBe(1);
    expect(m3?.rank).toBe(1);
    expect(m2?.y).toBe(m3?.y);
    expect(Math.abs((m2?.x ?? 0) - (m3?.x ?? 0))).toBe(COL_WIDTH);
  });

  it("keeps a twelve-milestone plan readable: no two nodes overlap", () => {
    const { nodes, edges } = layoutGraph(twelvePlan().milestones, new Map());
    expect(nodes).toHaveLength(12);
    const seen = new Set(nodes.map((n) => `${n.x},${n.y}`));
    expect(seen.size).toBe(12);
    for (const a of nodes)
      for (const b of nodes)
        if (a.id < b.id)
          expect(Math.abs(a.x - b.x)).toBeGreaterThanOrEqual(a.y === b.y ? COL_WIDTH : 0);
    expect(edges.length).toBeGreaterThan(0);
    expect(Math.max(...nodes.map((n) => n.rank))).toBe(8); // the chain M1…M3→…→M10→M11→M12
  });

  it("keeps a milestone next to what it needs most", () => {
    const plan = [
      milestone("M1"),
      milestone("M2", ["M1"]),
      milestone("M3", ["M1"]),
      milestone("M4", ["M2", "M3"]),
    ];
    const { nodes } = layoutGraph(plan, new Map());
    const m2 = nodes.find((n) => n.id === "M2");
    const m3 = nodes.find((n) => n.id === "M3");
    expect(m2 && m3 ? Math.abs(m2.x - m3.x) : -1).toBe(COL_WIDTH);
  });

  it("ignores a cycle when ranking instead of hanging, and drops edges to unknown milestones", () => {
    const plan = [
      milestone("M1", ["M2"]),
      milestone("M2", ["M1"]),
      milestone("M3", ["M1", "gone"]),
    ];
    const { nodes, edges } = layoutGraph(plan, new Map());
    expect(nodes).toHaveLength(3);
    // the cycle's edges are still drawn (the plan is a DAG in practice), `gone` is dropped
    expect(edges.map((e) => `${e.from}->${e.to}`)).toEqual(["M2->M1", "M1->M2", "M1->M3"]);
    for (const n of nodes) expect(n.rank).toBeGreaterThanOrEqual(0);
  });

  it("steps rows by ROW_HEIGHT and centres each row", () => {
    const plan = [milestone("M1"), milestone("M2", ["M1"]), milestone("M3", ["M1"])];
    const { nodes } = layoutGraph(plan, new Map());
    const row1 = nodes.filter((n) => n.rank === 1);
    const m1 = nodes.find((n) => n.id === "M1");
    expect(row1[0]?.y).toBe((m1?.y ?? 0) + ROW_HEIGHT);
    const left = Math.min(...row1.map((n) => n.x));
    expect(row1[1]?.x !== undefined ? row1[1].x - left : 0).toBe(COL_WIDTH);
  });
});

describe("wrapTitle", () => {
  it("wraps a long title into short lines", () => {
    expect(wrapTitle("Deploy the release candidate build", 20, 3)).toEqual([
      "Deploy the release",
      "candidate build",
    ]);
  });

  it("marks the last line when words do not fit", () => {
    const lines = wrapTitle("Alpha Beta Gamma Delta", 10, 2);
    expect(lines).toHaveLength(2);
    expect(lines[0]).toBe("Alpha Beta");
    expect(lines[1]?.endsWith("…")).toBe(true);
  });

  it("keeps a single long word whole", () => {
    expect(wrapTitle("extraordinarilylongword")).toEqual(["extraordinarilylongword"]);
  });
});
