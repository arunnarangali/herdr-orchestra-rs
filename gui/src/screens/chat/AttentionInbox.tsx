import { Link } from "react-router-dom";
import { NOTIFICATIONS_PATH } from "../../app/paths";
import { AttentionCard } from "../../components/AttentionCard";
import { useAnswerItemPrompt, useAttention, useRecheckItemPrompt } from "../../components/Attention";
import { Badge } from "../../ui/Badge";
import { Card, CardHead } from "../../ui/Card";
import { Icon } from "../../ui/Icon";

/** The right-column inbox: what waits now, then the few items this app just resolved. */
export function AttentionInbox() {
  const { waitingItems, resolvedItems, openItem } = useAttention();
  const answerPrompt = useAnswerItemPrompt();
  const recheckPrompt = useRecheckItemPrompt();
  const resolved = resolvedItems.slice(0, 3);
  return (
    <Card as="section" className="inbox" aria-labelledby="inbox-title">
      <CardHead title={<span id="inbox-title">Attention Inbox ({waitingItems.length})</span>} icon={<Icon name="bell" />}>
        <Link className="inbox__all" to={NOTIFICATIONS_PATH}>
          View all
        </Link>
      </CardHead>
      <div className="inbox__body">
        {waitingItems.length === 0 && (
          <div className="inbox__empty">
            <Icon name="check" />
            <strong>All clear</strong>
            <span>Nothing needs your attention right now.</span>
          </div>
        )}
        {waitingItems.length > 0 && (
          <ul className="att-list" aria-label="Waiting items">
            {waitingItems.map((item) => (
              <AttentionCard key={item.uid} item={item} onReview={openItem} onAnswerPrompt={answerPrompt} onRecheckPrompt={recheckPrompt} />
            ))}
          </ul>
        )}
        {resolved.length > 0 && (
          <div className="inbox__resolved">
            <h3 className="inbox__resolved-title">Recently resolved</h3>
            <ul>
              {resolved.map((item) => (
                <li key={item.uid}>
                  <Icon name="check" />
                  <span>{item.title}</span>
                  <Badge>{item.resolution}</Badge>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </Card>
  );
}
