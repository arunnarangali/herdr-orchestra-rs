import { useStream } from "../app/connection";
import "./components.css";

const TEXT = {
  idle: "Live updates: off",
  connecting: "Live updates: connecting…",
  connected: "Live updates: on",
  reconnecting: "Live updates: connection lost, reconnecting…",
  refused: "Live updates: refused (check the token)",
} as const;

/** Whether the screen is receiving the live stream. */
export function LiveStatus() {
  const { status } = useStream();
  return (
    <span className={`live live--${status}`} role="status" data-stream={status}>
      {TEXT[status]}
    </span>
  );
}
