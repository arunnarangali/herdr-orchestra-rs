import type { IconName } from "../../ui/Icon";
import type { Tone } from "../../ui/Button";

export interface KindLook {
  label: string;
  chip: string;
  tone: Tone;
  icon: IconName;
}

/** The server's attention kinds, in the order the prototype lists them. */
export const KIND_ORDER = ["question", "held", "failed", "blocked", "wake_unread", "plan_approval"] as const;

export const KINDS: Record<string, KindLook> = {
  question: { label: "Coder Question", chip: "Questions", tone: "cyan", icon: "question" },
  held: { label: "Risk Hold", chip: "Risk Holds", tone: "amber", icon: "warning" },
  failed: { label: "Verification Failure", chip: "Failures", tone: "coral", icon: "warning" },
  blocked: { label: "Permission Prompt Blocked", chip: "Blocked Prompts", tone: "coral", icon: "warning" },
  wake_unread: { label: "Unacknowledged Lead Wake-Up", chip: "Lead Wake-ups", tone: "violet", icon: "bell" },
  plan_approval: { label: "Plan Approval", chip: "Plan Approvals", tone: "cyan", icon: "question" },
};

export function kindLook(kind: string): KindLook {
  return KINDS[kind] ?? { label: kind, chip: kind, tone: "neutral", icon: "bell" };
}
