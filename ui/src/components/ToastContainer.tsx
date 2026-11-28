import { AlertTriangle, CheckCircle2, Info, X, XCircle } from "lucide-react";
import { useAppStore } from "../store/appStore";
import type { ToastVariant } from "../lib/types";

const VARIANT_STYLES: Record<
  ToastVariant,
  { border: string; iconColor: string; Icon: typeof Info }
> = {
  info: {
    border: "border-cyan-500/30",
    iconColor: "text-cyan-400",
    Icon: Info,
  },
  success: {
    border: "border-emerald-500/30",
    iconColor: "text-emerald-400",
    Icon: CheckCircle2,
  },
  warning: {
    border: "border-amber-500/30",
    iconColor: "text-amber-400",
    Icon: AlertTriangle,
  },
  error: {
    border: "border-red-500/35",
    iconColor: "text-red-400",
    Icon: XCircle,
  },
};

export function ToastContainer() {
  const { toasts, dismissToast } = useAppStore();

  if (toasts.length === 0) return null;

  return (
    <div
      aria-live="polite"
      className="fixed bottom-20 left-1/2 -translate-x-1/2 z-50 flex flex-col-reverse items-center gap-2 w-auto min-w-[260px] max-w-[min(420px,calc(100vw-2rem))] pointer-events-none"
    >
      {toasts.map((toast) => {
        const cfg = VARIANT_STYLES[toast.variant];
        const Icon = cfg.Icon;
        return (
          <div
            key={toast.id}
            className={`pointer-events-auto w-full px-3.5 py-2.5 rounded-xl bg-[#0D121B] border ${cfg.border} shadow-[0_14px_34px_rgba(0,0,0,0.8)] flex items-start gap-3 animate-fade-in`}
          >
            <Icon className={`w-4 h-4 mt-0.5 shrink-0 ${cfg.iconColor}`} />
            <div className="flex-1 min-w-0">
              <div className="text-[13px] font-semibold text-[var(--text-primary)] leading-snug">
                {toast.title}
              </div>
              {toast.description && (
                <div className="text-[11px] text-[var(--text-secondary)] mt-0.5 leading-relaxed break-words">
                  {toast.description}
                </div>
              )}
            </div>
            <button
              type="button"
              onClick={() => dismissToast(toast.id)}
              aria-label="Dismiss notification"
              className="text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors p-0.5 rounded cursor-pointer"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
