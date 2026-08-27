import { useToasts, type Tone } from "./toastStore";

const TONE: Record<Tone, string> = {
  plain: "bg-raised text-fg ring-line-strong",
  ok: "bg-raised text-keep ring-keep/40",
  drop: "bg-raised text-drop ring-drop/40",
};

/** 오른쪽 아래에 쌓인다. 누르면 바로 사라진다. */
export default function Toasts() {
  const toasts = useToasts((s) => s.toasts);
  const dismiss = useToasts((s) => s.dismiss);
  if (toasts.length === 0) return null;
  return (
    <div className="fixed right-4 bottom-12 z-[95] flex flex-col items-end gap-2 pointer-events-none">
      {toasts.map((t) => (
        <button
          key={t.id}
          onClick={() => dismiss(t.id)}
          role="status"
          className={`pointer-events-auto max-w-[360px] text-left px-3 py-2 rounded-lg shadow-lg ring-1 text-[12.5px] leading-snug ${TONE[t.tone]}`}
        >
          {t.text}
        </button>
      ))}
    </div>
  );
}
