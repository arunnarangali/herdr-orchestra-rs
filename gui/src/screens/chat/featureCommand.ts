import type { ChatBody } from "../../api/types";

/** The command that turns plain words into a feature request. */
export const FEAT_COMMAND = "/orch feat";
export const FEAT_HELP = "Write a feature request in your own words: the Lead fills in the details.";

/** What the developer is told when they type the command and nothing else. */
export const FEAT_NEEDS_TEXT =
  "Add a description after /orch feat: say in your own words what you want built and why, for example “/orch feat let people log in with a magic link”.";

/** Between what the developer typed and the Lead's instructions in the stored text. */
const SEPARATOR = "\n\n---\n";

/** Written to the Lead after the developer's words (the chat shows only the words). */
const INSTRUCTIONS = [
  "This is a feature request in plain words (the /orch feat command), not a filled-in form.",
  "Turn it into the structured request: Feature, Why, Must, Must not, Already decided, Done when, Mode.",
  "Feature and Why are required. Work out the rest from what I wrote and from the project; ask me only for what you truly cannot infer.",
  "Show me the structured request here in the chat so I can see what you understood, and wait for my yes before you plan.",
].join("\n");

/** The words after `/orch feat`, or null when the text is not that command. Case-insensitive. */
export function featureWords(text: string | null | undefined): string | null {
  const t = (text ?? "").trim();
  const m = /^\/orch\s+feat(?=\s|$)/i.exec(t);
  return m ? t.slice(m[0].length).trim() : null;
}

/** Typing so far could still become the command (`/`, `/or`, `/orch f`…): offer it. */
export function suggestsFeat(text: string): boolean {
  const t = text.replace(/^\s+/, "").toLowerCase();
  return t.startsWith("/") && t.length < FEAT_COMMAND.length && FEAT_COMMAND.startsWith(t);
}

/** The message to send for typed text: a feature request in plain words, or the text as it is. */
export function bodyForTyped(text: string): ChatBody {
  const words = featureWords(text);
  if (words === null || words === "") return { kind: "message", text, request: null };
  return { kind: "message", text: `${FEAT_COMMAND} ${words}${SEPARATOR}${INSTRUCTIONS}`, request: null };
}

/** What a chat bubble shows for a stored text: the developer's own words, not the instructions. */
export function shownText(text: string): string {
  if (typeof text !== "string") return text;
  const i = text.indexOf(SEPARATOR);
  return i > 0 && featureWords(text) !== null ? text.slice(0, i) : text;
}
