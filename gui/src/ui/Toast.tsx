import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import type { Tone } from "./Button";
import { Icon, type IconName } from "./Icon";

interface ToastItem {
  id: number;
  message: string;
  tone: Tone;
  icon: IconName;
}

export interface ToastOptions {
  tone?: Tone;
  icon?: IconName;
}

type Show = (message: string, options?: ToastOptions) => void;

const Ctx = createContext<Show>(() => {});

/** Show a short message that goes away by itself. Outside a provider it does nothing. */
export function useToast(): Show {
  return useContext(Ctx);
}

const LIFETIME_MS = 3600;

/** Hosts the toasts: a polite live region, so a screen reader announces each one. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const next = useRef(0);
  const show = useCallback<Show>((message, { tone = "cyan", icon = "check" } = {}) => {
    const id = ++next.current;
    setItems((prev) => [...prev, { id, message, tone, icon }]);
    setTimeout(() => setItems((prev) => prev.filter((t) => t.id !== id)), LIFETIME_MS);
  }, []);
  const value = useMemo(() => show, [show]);
  return (
    <Ctx.Provider value={value}>
      {children}
      <div id="toasts" role="status" aria-live="polite">
        {items.map((t) => (
          <div key={t.id} className={`toast tone-${t.tone}`}>
            <Icon name={t.icon} />
            <span>{t.message}</span>
          </div>
        ))}
      </div>
    </Ctx.Provider>
  );
}
