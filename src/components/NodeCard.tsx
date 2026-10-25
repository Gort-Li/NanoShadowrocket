import { Gauge, Pencil, Trash2 } from "lucide-react";
import { cn } from "../lib/cn";
import { formatLatency, latencyTone } from "../lib/format";
import type { Node } from "../lib/types";

const TONE_CLASS = {
  ok: "bg-ok shadow-[0_0_8px_rgba(48,209,88,0.8)]",
  warn: "bg-warn shadow-[0_0_8px_rgba(255,214,10,0.7)]",
  bad: "bg-danger shadow-[0_0_8px_rgba(255,69,58,0.7)]",
  none: "bg-ink-faint/60",
};

export default function NodeCard({
  node,
  selected,
  testing,
  onSelect,
  onEdit,
  onDelete,
  onTest,
}: {
  node: Node;
  selected: boolean;
  testing: boolean;
  onSelect: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onTest: () => void;
}) {
  const tone = latencyTone(node.latencyMs);
  const unsupported = node.transport !== "tcp";
  const securityLabel =
    node.security === "chacha20-poly1305"
      ? "chacha20"
      : node.security === "legacy"
        ? "legacy"
        : "aes-128-gcm";

  return (
    <div
      className={cn(
        "group relative flex items-center gap-3 rounded-card border px-4 py-3 transition-all duration-200",
        "hover:-translate-y-px",
        selected
          ? "border-accent/35 bg-[#101E2C] hover:bg-[#13222F]"
          : "border-hairline bg-surface/70 hover:bg-white/[0.055]"
      )}
    >
      {selected && (
        <span className="absolute left-0 top-1/2 h-8 w-[3px] -translate-y-1/2 rounded-r-full bg-accent shadow-[0_0_10px_rgba(10,132,255,0.85)]" />
      )}

      <button
        type="button"
        role="radio"
        aria-checked={selected}
        aria-label={`选择 ${node.name}`}
        onClick={onSelect}
        className="grid h-5 w-5 shrink-0 place-items-center rounded-full border transition-colors"
        style={{
          borderColor: selected ? "#0A84FF" : "rgba(255,255,255,0.18)",
        }}
      >
        {selected && <span className="h-2 w-2 rounded-full bg-accent" />}
      </button>

      <button
        type="button"
        onClick={onSelect}
        className="min-w-0 flex-1 cursor-pointer text-left"
      >
        <div className="flex items-center gap-2">
          <span className="truncate text-[13px] font-medium text-ink">{node.name}</span>
          {unsupported && (
            <span className="rounded-md border border-warn/30 bg-warn/10 px-1.5 py-[1px] text-[9px] text-warn">
              {node.transport} 不支持
            </span>
          )}
        </div>
        <p className="mt-0.5 truncate font-mono text-[11px] text-ink-faint">
          {node.host}:{node.port}
        </p>
      </button>

      <div className="hidden shrink-0 items-center gap-1.5 sm:flex">
        <span className="rounded-md border border-hairline bg-white/[0.03] px-1.5 py-[2px] text-[10px] text-ink-faint">
          {securityLabel}
        </span>
        {node.udp && (
          <span className="rounded-md border border-hairline bg-white/[0.03] px-1.5 py-[2px] text-[10px] text-ink-faint">
            UDP
          </span>
        )}
        <span
          className={cn(
            "rounded-md border px-1.5 py-[2px] text-[10px]",
            unsupported
              ? "border-warn/30 bg-warn/10 text-warn"
              : "border-hairline bg-white/[0.03] text-ink-faint"
          )}
        >
          {node.transport}
        </span>
      </div>

      <div className="flex w-[74px] shrink-0 items-center justify-end gap-1.5">
        <span className={cn("h-1.5 w-1.5 rounded-full", TONE_CLASS[tone])} />
        <span
          className={cn(
            "font-mono text-[11px]",
            tone === "ok" && "text-ok",
            tone === "warn" && "text-warn",
            tone === "bad" && "text-danger",
            tone === "none" && "text-ink-faint"
          )}
        >
          {testing ? "测速中" : formatLatency(node.latencyMs)}
        </span>
      </div>

      <div className="flex shrink-0 items-center gap-0.5 opacity-45 transition-opacity group-hover:opacity-100">
        <button
          type="button"
          title="测试延迟"
          aria-label="测试延迟"
          onClick={onTest}
          className="grid h-7 w-7 place-items-center rounded-lg text-ink-faint transition-colors hover:bg-white/[0.07] hover:text-ink"
        >
          <Gauge size={14} />
        </button>
        <button
          type="button"
          title="编辑节点"
          aria-label="编辑节点"
          onClick={onEdit}
          className="grid h-7 w-7 place-items-center rounded-lg text-ink-faint transition-colors hover:bg-white/[0.07] hover:text-ink"
        >
          <Pencil size={13} />
        </button>
        <button
          type="button"
          title="删除节点"
          aria-label="删除节点"
          onClick={onDelete}
          className="grid h-7 w-7 place-items-center rounded-lg text-ink-faint transition-colors hover:bg-danger/15 hover:text-danger"
        >
          <Trash2 size={13} />
        </button>
      </div>
    </div>
  );
}
