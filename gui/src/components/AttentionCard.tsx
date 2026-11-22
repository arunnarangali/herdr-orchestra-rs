import type { TaggedItem } from "./attentionStore";
import { Badge } from "../ui/Badge";
import { Button, type Tone } from "../ui/Button";
import { Card } from "../ui/Card";
import { Icon, type IconName } from "../ui/Icon";
import "./components.css";

const KINDS: Record<string, { tone: Tone; icon: IconName }> = {
  held: { tone: "amber", icon: "warning" },
  blocked: { tone: "amber", icon: "warning" },
  question: { tone: "cyan", icon: "question" },
  plan_approval: { tone: "cyan", icon: "question" },
  failed: { tone: "coral", icon: "warning" },
  wake_unread: { tone: "violet", icon: "bell" },
};

/** The tone and icon an attention item is shown with. */
export function attentionLook(kind: string): { tone: Tone; icon: IconName } {
  return KINDS[kind] ?? { tone: "neutral", icon: "bell" };
}

export interface AttentionCardProps {
  item: TaggedItem;
  /** Open the item (the popup where it is answered). */
  onReview: (item: TaggedItem) => void;
}

/**
 * One item that needs the developer, as the notification drawer and the Attention Center show
 * it: what it is, where it comes from, and a way to act on it.
 */
export function AttentionCard({ item, onReview }: AttentionCardProps) {
  const { tone, icon } = attentionLook(item.kind);
  const body = item.detail ?? item.prompt?.text;
  return (
    <Card as="li" tone={tone} className="att" data-attention={item.id}>
      <div className="att__head">
        <span className="att__icon">
          <Icon name={icon} size={20} />
        </span>
        <h3 className="att__title">{item.title}</h3>
        {item.task && (
          <Badge tone={tone} mono>
            {item.task}
          </Badge>
        )}
      </div>
      <div className="att__meta">
        {item.projectName && <Badge>{item.projectName}</Badge>}
        {item.agent && <Badge tone="cyan">{item.agent}</Badge>}
        <Badge tone={tone}>{item.kind}</Badge>
      </div>
      {body && <p className="att__body">{body}</p>}
      <div className="att__actions">
        <Button tone={tone} aria-label={`Review ${item.title}`} onClick={() => onReview(item)}>
          Review
        </Button>
      </div>
    </Card>
  );
}
