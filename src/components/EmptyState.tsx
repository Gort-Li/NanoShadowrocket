import type { LucideIcon } from "lucide-react";

export default function EmptyState({
  icon: Icon,
  title,
  description,
  hint,
}: {
  icon: LucideIcon;
  title: string;
  description: string;
  hint?: string;
}) {
  return (
    <div className="card flex flex-col items-center justify-center gap-3 px-6 py-12 text-center">
      <div className="grid h-12 w-12 place-items-center rounded-2xl border border-hairline bg-white/[0.03] text-ink-faint">
        <Icon size={20} strokeWidth={1.6} />
      </div>
      <p className="text-sub text-ink">{title}</p>
      <p className="max-w-[420px] text-[12px] leading-5 text-ink-muted">{description}</p>
      {hint && (
        <p className="mt-1 rounded-lg bg-white/[0.03] px-2.5 py-1 font-mono text-[11px] text-ink-faint">
          {hint}
        </p>
      )}
    </div>
  );
}
