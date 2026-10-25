import type { ReactNode } from "react";

export default function StatCard({
  label,
  value,
  unit,
  icon,
  tone = "default",
  hint,
}: {
  label: string;
  value: string;
  unit: string;
  icon: ReactNode;
  tone?: "default" | "up" | "down";
  hint?: string;
}) {
  const iconTone =
    tone === "up" ? "text-ok" : tone === "down" ? "text-info" : "text-ink-muted";
  return (
    <div className="card flex flex-1 items-center gap-3 px-4 py-3">
      <div className="grid h-9 w-9 place-items-center rounded-xl bg-white/[0.04]">{icon}</div>
      <div className="min-w-0">
        <p className="text-[11px] text-ink-faint">{label}</p>
        <p className={`mt-0.5 font-mono text-[15px] leading-5 ${iconTone}`}>
          {value}
          <span className="ml-1 text-[11px] font-normal text-ink-faint">{unit}</span>
        </p>
        {hint && <p className="mt-0.5 text-[10px] text-ink-faint/80">{hint}</p>}
      </div>
    </div>
  );
}
