/** What a milestone in this state is waiting for from the developer, or null. */
export function needsHuman(state: string, reason: string): { label: string; text: string } | null {
  const why = reason.trim();
  switch (state) {
    case "needs-answer":
      return { label: "Needs your answer", text: why || "The coder asked a question." };
    case "paused":
      return { label: "Paused", text: why || "Paused: answer its question or dispatch it again." };
    case "held":
      return { label: "Held for your approval", text: why || "Held: see the reasons, then approve." };
    case "failed":
      return { label: "Failed", text: why || "Failed without a reason." };
    case "conflict":
      return { label: "Merge conflict", text: why || "The merge conflicted." };
    case "needs-fix":
      return { label: "Needs a fix", text: why || "A fix is needed." };
    default:
      return null;
  }
}
