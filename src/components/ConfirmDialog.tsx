import { AlertTriangle } from "lucide-react";
import { useEffect } from "react";
import { createPortal } from "react-dom";
import Button from "./Button";

export default function ConfirmDialog({
  open,
  title,
  description,
  confirmText = "确认",
  cancelText = "取消",
  danger = true,
  loading = false,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  description: string;
  confirmText?: string;
  cancelText?: string;
  danger?: boolean;
  loading?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onCancel]);

  if (!open) return null;

  return createPortal(
    <div className="fixed inset-0 z-[60] grid place-items-center px-4">
      <button
        type="button"
        aria-label="取消"
        onClick={onCancel}
        className="absolute inset-0 cursor-default bg-black/60 backdrop-blur-[3px] animate-overlay-in"
      />
      <div className="animate-fade-up relative w-[380px] max-w-full rounded-card border border-hairline bg-[#171B21] p-5 shadow-card">
        <div className="flex items-start gap-3">
          <div
            className={`grid h-9 w-9 shrink-0 place-items-center rounded-xl ${
              danger ? "bg-danger/15 text-danger" : "bg-accent-soft text-accent"
            }`}
          >
            <AlertTriangle size={17} />
          </div>
          <div className="min-w-0">
            <h3 className="text-sub text-ink">{title}</h3>
            <p className="mt-1.5 text-[12px] leading-5 text-ink-muted">{description}</p>
          </div>
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <Button onClick={onCancel}>{cancelText}</Button>
          <Button
            variant={danger ? "danger" : "primary"}
            loading={loading}
            onClick={onConfirm}
          >
            {confirmText}
          </Button>
        </div>
      </div>
    </div>,
    document.body
  );
}
