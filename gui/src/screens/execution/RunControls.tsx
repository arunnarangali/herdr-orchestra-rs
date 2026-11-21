import { useState } from "react";
import { useApi } from "../../app/connection";
import { ActionResult } from "../../components/ActionResult";
import { useAction } from "../../components/useAction";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { ExecIcon } from "./icons";

type Which = "pause" | "resume" | "stop";

const COPY: Record<Which, { button: string; icon: string; title: string; body: string; confirm: string; destructive: boolean }> = {
  pause: {
    button: "Pause",
    icon: "pause",
    title: "Pause the run?",
    body: "No new work starts. Milestones that are running finish their current step.",
    confirm: "Pause",
    destructive: false,
  },
  resume: {
    button: "Resume",
    icon: "play",
    title: "Resume the run?",
    body: "The run carries on: ready milestones start again.",
    confirm: "Resume",
    destructive: false,
  },
  stop: {
    button: "Stop",
    icon: "stop",
    title: "Stop the run?",
    body: "Each running coder's work is saved and the event manager ends. Nothing new starts until you start the event manager again.",
    confirm: "Stop the run",
    destructive: true,
  },
};

/** Why a control does not apply right now (null when it does). Unknown state allows all: the server decides. */
export function disabledReason(which: Which, control: string | null): string | null {
  if (control === "stopping") return "The run is already stopping.";
  if (control === "stopped") return which === "resume" ? "The run is stopped: start the event manager again." : "The run is already stopped.";
  if (which === "pause" && control === "paused") return "The run is already paused.";
  if (which === "resume" && control === "running") return "The run is not paused.";
  return null;
}

/**
 * Pause, Resume and Stop, each behind a confirmation. `control` is the run's control state as
 * the stream last said it; `onControl` records what the server answered.
 */
export function RunControls({
  control,
  onControl,
  onChange,
}: {
  control: string | null;
  onControl: (state: string) => void;
  onChange: () => void;
}) {
  const api = useApi();
  const { outcome, run, pending } = useAction(onChange);
  const [asking, setAsking] = useState<Which | null>(null);

  const confirm = async () => {
    const which = asking;
    setAsking(null);
    if (!which) return;
    const call = which === "pause" ? api.pauseRun : which === "resume" ? api.resumeRun : api.stopRun;
    await run(async () => {
      const r = await call();
      onControl(r.run_control);
      return r;
    });
  };

  return (
    <>
      <div className="progress__actions">
        {(["pause", "resume", "stop"] as const).map((w) => {
          const reason = disabledReason(w, control);
          return (
            <span key={w} className="control">
              <button
                type="button"
                className={w === "stop" ? "btn btn--coral" : "btn"}
                aria-haspopup="dialog"
                disabled={pending || reason !== null}
                aria-describedby={reason ? `why-${w}` : undefined}
                onClick={() => setAsking(w)}
              >
                <ExecIcon name={COPY[w].icon} />
                {COPY[w].button}
              </button>
              {reason && (
                <span id={`why-${w}`} className="muted control-why">
                  {reason}
                </span>
              )}
            </span>
          );
        })}
        <span className="control">
          <button type="button" className="btn btn--emerald btn--solid" disabled aria-describedby="why-herdr">
            <ExecIcon name="external" />
            Open in Herdr
          </button>
          <span id="why-herdr" className="muted control-why">
            Not available yet
          </span>
        </span>
      </div>
      <ActionResult outcome={outcome} />
      {asking && (
        <ConfirmDialog
          open
          title={COPY[asking].title}
          confirmLabel={COPY[asking].confirm}
          destructive={COPY[asking].destructive}
          onConfirm={() => void confirm()}
          onCancel={() => setAsking(null)}
        >
          {COPY[asking].body}
        </ConfirmDialog>
      )}
    </>
  );
}
