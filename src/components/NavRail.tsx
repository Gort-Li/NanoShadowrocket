import { Gauge, ListFilter, Server, Settings } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { cn } from "../lib/cn";

export type PageKey = "home" | "nodes" | "rules" | "settings";

type NavItem = {
  key: PageKey;
  label: string;
  icon: LucideIcon;
};

const ITEMS: NavItem[] = [
  { key: "home", label: "首页", icon: Gauge },
  { key: "nodes", label: "节点", icon: Server },
  { key: "rules", label: "规则", icon: ListFilter },
  { key: "settings", label: "设置", icon: Settings },
];

export default function NavRail({
  active,
  onChange,
}: {
  active: PageKey;
  onChange: (key: PageKey) => void;
}) {
  return (
    <nav className="z-30 flex w-16 shrink-0 flex-col items-center gap-1 border-r border-hairline bg-[#0D1015] py-3">
      {ITEMS.map(({ key, label, icon: Icon }) => {
        const isActive = key === active;
        return (
          <button
            key={key}
            type="button"
            title={label}
            aria-current={isActive ? "page" : undefined}
            onClick={() => onChange(key)}
            className={cn(
              "group relative grid h-12 w-12 place-items-center rounded-xl transition-all duration-200",
              isActive
                ? "bg-accent-soft text-accent shadow-[inset_0_0_0_1px_rgba(10,132,255,0.32)]"
                : "text-ink-faint hover:bg-white/[0.05] hover:text-ink-muted active:scale-[0.96]"
            )}
          >
            {isActive && (
              <span className="absolute -left-2 h-5 w-[3px] rounded-r-full bg-accent shadow-[0_0_10px_rgba(10,132,255,0.9)]" />
            )}
            <Icon size={19} strokeWidth={isActive ? 2 : 1.7} />
            <span className="pointer-events-none absolute left-[56px] z-50 whitespace-nowrap rounded-md border border-hairline bg-elevated px-2 py-1 text-[11px] text-ink opacity-0 shadow-card transition-opacity group-hover:opacity-100">
              {label}
            </span>
          </button>
        );
      })}

      <div className="mt-auto flex flex-col items-center gap-2 pb-1">
        <span className="h-1.5 w-1.5 rounded-full bg-ink-faint/60" />
        <span className="text-[9px] tracking-widest text-ink-faint/70">PC</span>
      </div>
    </nav>
  );
}
