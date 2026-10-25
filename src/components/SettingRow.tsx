import type { ReactNode } from "react";
import { cn } from "../lib/cn";

export default function SettingRow({
  label,
  description,
  children,
  danger,
  onClick,
}: {
  label: string;
  description?: string;
  children?: ReactNode;
  danger?: boolean;
  onClick?: () => void;
}) {
  const interactive = Boolean(onClick);
  return (
    <div
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onClick={onClick}
      onKeyDown={(e) => {
        if (interactive && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          onClick?.();
        }
      }}
      className={cn(
        "flex min-h-[52px] items-center justify-between gap-4 px-4 py-2.5 transition-colors",
        interactive && "cursor-pointer hover:bg-white/[0.03]"
      )}
    >
      <div className="min-w-0">
        <p className={cn("text-[13px]", danger ? "text-danger" : "text-ink")}>{label}</p>
        {description && (
          <p className="mt-0.5 text-[11px] leading-4 text-ink-faint">{description}</p>
        )}
      </div>
      {children && <div className="flex shrink-0 items-center gap-2">{children}</div>}
    </div>
  );
}
