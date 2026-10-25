import { AlertCircle, CheckCircle2, Info, X } from "lucide-react";
import { cn } from "../lib/cn";
import { useAppStore } from "../store/useAppStore";

const ICONS = {
  ok: CheckCircle2,
  error: AlertCircle,
  info: Info,
};

export default function Toaster() {
  const toasts = useAppStore((s) => s.toasts);
  const dismiss = useAppStore((s) => s.dismissToast);

  return (
    <div className="pointer-events-none fixed bottom-5 left-1/2 z-[100] flex w-[min(420px,92vw)] -translate-x-1/2 flex-col items-center gap-2">
      {toasts.map((toast) => {
        const Icon = ICONS[toast.kind];
        return (
          <div
            key={toast.id}
            className={cn(
              "pointer-events-auto flex w-full animate-fade-up items-center gap-2.5 rounded-xl border px-3.5 py-2.5",
              "bg-[#171B21]/95 text-[12px] shadow-card backdrop-blur-xl",
              toast.kind === "error"
                ? "border-danger/30 text-danger"
                : toast.kind === "ok"
                  ? "border-ok/25 text-ink"
                  : "border-hairline text-ink"
            )}
          >
            <Icon
              size={15}
              className={cn(
                "shrink-0",
                toast.kind === "ok" && "text-ok",
                toast.kind === "error" && "text-danger",
                toast.kind === "info" && "text-info"
              )}
            />
            <span className="min-w-0 flex-1 leading-5">{toast.message}</span>
            <button
              type="button"
              aria-label="关闭提示"
              onClick={() => dismiss(toast.id)}
              className="grid h-5 w-5 shrink-0 place-items-center rounded-md text-ink-faint transition-colors hover:bg-white/[0.06] hover:text-ink"
            >
              <X size={12} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
