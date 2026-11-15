import type { AgentKindInfo } from "../../api/types";

/** Plain words for what is wrong with an agent kind, and what to do about it. Empty when fine. */
export function agentProblems(a: AgentKindInfo): string[] {
  const name = a.display_name ?? a.kind;
  const out: string[] = [];
  if (!a.installed) {
    out.push(`${name} is not installed. Install it so \`${a.launch}\` works in a terminal, then run \`orch doctor\` to check.`);
    return out;
  }
  if (a.login.state === "not authenticated") {
    out.push(`${name} is not logged in${a.login.reason ? ` (${a.login.reason})` : ""}. Log in with the tool itself, then run \`orch doctor\`.`);
  } else if (a.login.state === "unknown") {
    out.push(`The login state of ${name} could not be read here${a.login.reason ? ` (${a.login.reason})` : ""}. Run \`orch doctor\` to check it.`);
  }
  if (a.skills.status === "missing") {
    out.push(`The orch skills are missing for ${name}: ${a.skills.problems.join(", ") || "no files found"}. Run \`orch doctor\` for the fix.`);
  } else if (a.skills.status === "outdated") {
    out.push(`The orch skills for ${name} are outdated: ${a.skills.problems.join(", ")}. Run \`orch doctor\` for the fix.`);
  }
  if (!a.herdr_detect) {
    out.push(`Herdr cannot tell when ${name} is idle, working or blocked, so the run watches it less closely.`);
  }
  if (!a.verified) {
    out.push(`The launch command for ${name} has not been checked against the real tool.`);
  }
  return out;
}

const RANK = ["small", "medium", "high"];
/** Position of a tier in small < medium < high; 0 when absent. */
export function tierRank(t: string | null | undefined): number {
  const i = t ? RANK.indexOf(t) : -1;
  return i + 1;
}
