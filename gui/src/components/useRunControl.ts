import { useCallback, useState } from "react";
import { useStream } from "../app/connection";

/**
 * The run's control state (`running`, `paused`, `stopping`, `stopped`) as the live stream last
 * said it. The read API has no such field, so before the stream's first `run-state` message it
 * is null (unknown). `set` records what an action answered until a newer message arrives.
 */
export function useRunControl(): { control: string | null; set: (state: string) => void } {
  const { messages } = useStream();
  const last = [...messages["run-state"]].reverse().find((m) => m.data.run_control);
  const lastId = last?.id ?? null;
  const [override, setOverride] = useState<{ value: string; after: string | null } | null>(null);
  const set = useCallback((value: string) => setOverride({ value, after: lastId }), [lastId]);
  const control = override && override.after === lastId ? override.value : (last?.data.run_control ?? null);
  return { control, set };
}
