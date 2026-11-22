import type { AgentPrompt } from "../../api/types";
import { PromptAnswer } from "../../components/PromptAnswer";
import { Card, CardHead } from "../../ui/Card";
import { Icon } from "../../ui/Icon";

/**
 * The Lead is stopped at a question in its pane (a permission, a trust prompt…): the question
 * and one button per answer, so the developer answers it here and never opens Herdr. It shows
 * while the Lead is starting too (the first-run "trust this folder" question stops a start)
 * and goes away when the stream (or a fresh read) says the Lead carried on.
 */
export function LeadPrompt({
  prompt,
  onAnswer,
  onRecheck,
}: {
  prompt: AgentPrompt;
  onAnswer: (option: string) => Promise<unknown>;
  onRecheck: () => Promise<unknown>;
}) {
  return (
    <Card as="section" tone="amber" className="lead-prompt" aria-labelledby="lead-prompt-title" data-lead-prompt="">
      <CardHead level={3} icon={<Icon name="question" />} title={<span id="lead-prompt-title">The Lead is waiting for your answer</span>} />
      <PromptAnswer prompt={prompt} who="the Lead" onAnswer={onAnswer} onRecheck={onRecheck} />
    </Card>
  );
}
