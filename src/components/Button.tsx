import { Loader2 } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cn } from "../lib/cn";

type Variant = "primary" | "secondary" | "ghost" | "danger";

const VARIANTS: Record<Variant, string> = {
  primary:
    "bg-accent text-white shadow-[0_6px_20px_rgba(10,132,255,0.28)] hover:bg-[#2A96FF] active:bg-[#0A78E6]",
  secondary:
    "border border-hairline bg-white/[0.04] text-ink hover:bg-white/[0.08] active:bg-white/[0.05]",
  ghost: "text-ink-muted hover:bg-white/[0.06] hover:text-ink",
  danger:
    "border border-danger/30 bg-danger/10 text-danger hover:bg-danger/20 active:bg-danger/15",
};

export default function Button({
  variant = "secondary",
  loading = false,
  icon,
  children,
  className,
  disabled,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  loading?: boolean;
  icon?: ReactNode;
}) {
  return (
    <button
      type="button"
      {...rest}
      disabled={disabled || loading}
      className={cn(
        "inline-flex h-8 items-center justify-center gap-1.5 rounded-[10px] px-3 text-[12px] font-medium transition-all duration-150",
        "disabled:cursor-not-allowed disabled:opacity-50",
        VARIANTS[variant],
        className
      )}
    >
      {loading ? <Loader2 size={13} className="animate-spin" /> : icon}
      {children}
    </button>
  );
}
