import { X } from "lucide-react";
import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";

export default function Sheet({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  width = 430,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  // 用 Portal 挂到 body：页面根节点的入场动画带 transform，
  // 会让子级 fixed 元素相对页面而不是视口定位。
  return createPortal(
    <div className="fixed inset-0 z-50 flex justify-end">
      <button
        type="button"
        aria-label="关闭抽屉"
        onClick={onClose}
        className="absolute inset-0 cursor-default bg-black/55 backdrop-blur-[2px] animate-overlay-in"
      />
      <aside
        style={{ width }}
        className="sheet-in relative flex h-full max-w-[94vw] flex-col border-l border-hairline bg-[#12161B] shadow-[-24px_0_60px_rgba(0,0,0,0.45)]"
      >
        <header className="flex shrink-0 items-start justify-between gap-3 border-b border-hairline px-5 py-4">
          <div>
            <h2 className="text-sub text-ink">{title}</h2>
            {description && (
              <p className="mt-1 text-[11px] leading-4 text-ink-muted">{description}</p>
            )}
          </div>
          <button
            type="button"
            aria-label="关闭"
            onClick={onClose}
            className="grid h-7 w-7 shrink-0 place-items-center rounded-lg text-ink-faint transition-colors hover:bg-white/[0.06] hover:text-ink"
          >
            <X size={15} />
          </button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>

        {footer && (
          <footer className="flex shrink-0 items-center justify-end gap-2 border-t border-hairline px-5 py-3.5">
            {footer}
          </footer>
        )}
      </aside>
    </div>,
    document.body
  );
}
