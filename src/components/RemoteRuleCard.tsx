import { GripVertical, RefreshCw, Trash2 } from "lucide-react";
import { cn } from "../lib/cn";
import { formatRelativeTime } from "../lib/format";
import type { RemoteRule } from "../lib/types";
import { useRuleStore } from "../store/useRuleStore";
import Switch from "./Switch";

const STATUS_DOT = {
  ok: "bg-ok shadow-[0_0_8px_rgba(48,209,88,0.8)]",
  error: "bg-danger shadow-[0_0_8px_rgba(255,69,58,0.7)]",
  idle: "bg-ink-faint/60",
};

export default function RemoteRuleCard({
  remote,
  dragging,
  onDelete,
  onDragStart,
  onDragOver,
  onDrop,
  onDragEnd,
}: {
  remote: RemoteRule;
  dragging: boolean;
  onDelete: () => void;
  onDragStart: () => void;
  onDragOver: (event: React.DragEvent) => void;
  onDrop: () => void;
  onDragEnd: () => void;
}) {
  const updateRemote = useRuleStore((s) => s.updateRemote);
  const setRemoteInterval = useRuleStore((s) => s.setRemoteInterval);
  const setRemoteEnabled = useRuleStore((s) => s.setRemoteEnabled);

  return (
    <article
      draggable
      onDragStart={(event) => {
        event.dataTransfer.effectAllowed = "move";
        onDragStart();
      }}
      onDragOver={onDragOver}
      onDrop={(event) => {
        event.preventDefault();
        onDrop();
      }}
      onDragEnd={onDragEnd}
      className={cn(
        "card px-3.5 py-3 transition-all duration-200",
        dragging ? "opacity-45" : "hover:bg-white/[0.045]",
        !remote.enabled && "opacity-70"
      )}
    >
      <div className="flex items-start gap-2.5 max-[860px]:flex-wrap">
        <button
          type="button"
          aria-label="拖动调整顺序"
          title="拖动调整顺序"
          className="mt-1 cursor-grab text-ink-faint/70 transition-colors hover:text-ink-muted active:cursor-grabbing"
        >
          <GripVertical size={14} />
        </button>

        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className={cn("h-1.5 w-1.5 shrink-0 rounded-full", STATUS_DOT[remote.status])} />
            <span className="truncate text-[12px] text-ink">{remote.name}</span>
            {!remote.enabled && (
              <span className="shrink-0 rounded-md border border-hairline bg-white/[0.03] px-1.5 py-[1px] text-[9px] text-ink-faint">
                已停用
              </span>
            )}
            {remote.error && (
              <span className="shrink-0 rounded-md border border-danger/30 bg-danger/10 px-1.5 py-[1px] text-[9px] text-danger">
                更新失败
              </span>
            )}
          </div>
          <p className="mt-0.5 truncate font-mono text-[10px] text-ink-faint/90">{remote.url}</p>
          <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-ink-faint">
            <span>{remote.ruleCount.toLocaleString()} 条规则</span>
            {remote.ignoredCount > 0 && <span>已忽略 {remote.ignoredCount} 条</span>}
            <span>更新于 {formatRelativeTime(remote.updatedAt)}</span>
          </div>
          {remote.error && (
            <p className="mt-1.5 truncate text-[10px] text-danger/90">{remote.error}</p>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-1 max-[860px]:mt-2 max-[860px]:w-full max-[860px]:justify-end">
          <select
            aria-label="更新间隔"
            value={remote.intervalHours}
            onChange={(e) => void setRemoteInterval(remote.id, Number(e.target.value))}
            className="h-7 w-[78px] cursor-pointer rounded-lg border border-hairline bg-white/[0.03] px-1.5 text-[11px] text-ink-muted outline-none transition-colors hover:bg-white/[0.06] focus:border-accent/40"
          >
            {[0, 6, 12, 24, 48].map((hours) => (
              <option key={hours} value={hours}>
                {hours === 0 ? "手动" : `${hours} 小时`}
              </option>
            ))}
          </select>
          <button
            type="button"
            title="立即更新"
            aria-label="立即更新"
            onClick={() => void updateRemote(remote.id)}
            className="grid h-7 w-7 shrink-0 place-items-center rounded-lg text-ink-muted transition-colors hover:bg-white/[0.07] hover:text-accent"
          >
            <RefreshCw size={12} />
          </button>
          <button
            type="button"
            title="删除订阅"
            aria-label="删除订阅"
            onClick={onDelete}
            className="grid h-7 w-7 shrink-0 place-items-center rounded-lg text-ink-muted transition-colors hover:bg-danger/15 hover:text-danger"
          >
            <Trash2 size={12} />
          </button>
          <Switch
            checked={remote.enabled}
            label={`启用 ${remote.name}`}
            onChange={(next) => void setRemoteEnabled(remote.id, next)}
          />
        </div>
      </div>
    </article>
  );
}
