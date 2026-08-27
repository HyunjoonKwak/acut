import { CheckCircle2, AlertCircle, Info, X } from "lucide-react";
import { useToastStore } from "@/stores/toastStore";
import type { ToastType } from "@/stores/toastStore";

const ICONS: Record<ToastType, typeof Info> = {
  success: CheckCircle2,
  error: AlertCircle,
  info: Info,
};

const COLORS: Record<ToastType, string> = {
  success: "text-success border-success/30",
  error: "text-danger border-danger/30",
  info: "text-accent border-accent/30",
};

export function Toasts() {
  const toasts = useToastStore((s) => s.toasts);
  const dismiss = useToastStore((s) => s.dismiss);

  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2 max-w-sm">
      {toasts.map((t) => {
        const Icon = ICONS[t.type];
        return (
          <div
            key={t.id}
            className={`flex items-start gap-2 px-3 py-2.5 rounded-lg border bg-bg-elevated shadow-lg shadow-black/10 animate-slide-in ${COLORS[t.type]}`}
          >
            <Icon size={14} className="mt-0.5 shrink-0" />
            <p className="flex-1 text-[11px] text-text-primary leading-relaxed break-all">
              {t.message}
            </p>
            <button
              onClick={() => dismiss(t.id)}
              className="shrink-0 text-text-secondary hover:text-text-primary transition-colors"
            >
              <X size={12} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
