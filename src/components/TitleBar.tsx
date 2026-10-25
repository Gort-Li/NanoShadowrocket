import { useEffect, useState } from "react";
import { Copy, Minus, Square, X } from "lucide-react";
import { cn } from "../lib/cn";
import {
  closeWindow,
  isTauri,
  minimizeWindow,
  startDragging,
  toggleMaximizeWindow,
} from "../lib/tauri";

function ControlButton({
  label,
  onClick,
  danger,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "no-drag flex h-[38px] w-[46px] items-center justify-center text-ink-muted transition-colors",
        danger
          ? "hover:bg-danger hover:text-white"
          : "hover:bg-white/[0.08] hover:text-ink"
      )}
    >
      {children}
    </button>
  );
}

export default function TitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    let dispose: (() => void) | undefined;
    (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      setMaximized(await win.isMaximized());
      dispose = await win.onResized(async () => {
        setMaximized(await win.isMaximized());
      });
    })();
    return () => dispose?.();
  }, []);

  return (
    <header
      data-tauri-drag-region
      onMouseDown={(e) => {
        // WebView2 不支持 -webkit-app-region，交给 Tauri 处理拖拽
        if (e.button !== 0) return;
        if ((e.target as HTMLElement).closest("button")) return;
        void startDragging();
      }}
      className="relative z-40 flex h-[38px] shrink-0 items-center justify-between border-b border-hairline bg-[#0D1015]/90 backdrop-blur-xl"
    >
      <div data-tauri-drag-region className="pointer-events-none flex items-center gap-2.5 pl-3.5">
        <div className="grid h-[18px] w-[18px] place-items-center rounded-[6px] bg-gradient-to-br from-accent to-[#5AC8FA] shadow-[0_0_12px_rgba(10,132,255,0.55)]">
          <svg viewBox="0 0 24 24" className="h-3 w-3 text-white" fill="currentColor">
            <path d="M13 2 4.5 13.5H10l-1 8.5 9-12h-5.5L13 2Z" />
          </svg>
        </div>
        <span className="text-[11px] font-medium tracking-[0.14em] text-ink-faint uppercase">
          Nano Shadowrocket
        </span>
        <span className="ml-1 rounded-full border border-hairline bg-white/[0.04] px-1.5 py-[1px] text-[9px] font-medium text-ink-faint">
          v0.1.0
        </span>
      </div>

      <div className="flex h-full items-stretch">
        <ControlButton label="最小化" onClick={() => void minimizeWindow()}>
          <Minus size={14} strokeWidth={1.8} />
        </ControlButton>
        <ControlButton
          label={maximized ? "还原" : "最大化"}
          onClick={() => void toggleMaximizeWindow().then(setMaximized)}
        >
          {maximized ? (
            <Copy size={12} strokeWidth={1.8} className="-scale-x-100" />
          ) : (
            <Square size={11} strokeWidth={1.8} />
          )}
        </ControlButton>
        <ControlButton label="关闭" danger onClick={() => void closeWindow()}>
          <X size={15} strokeWidth={1.8} />
        </ControlButton>
      </div>
    </header>
  );
}
