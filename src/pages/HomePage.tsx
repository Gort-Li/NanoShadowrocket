import {
  ArrowDown,
  ArrowUp,
  Check,
  CircleSlash,
  Copy,
  ListFilter,
  Loader2,
  Power,
  Radio,
  Server,
  ShieldCheck,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import ActiveConnections from "../components/ActiveConnections";
import TrafficChart from "../components/TrafficChart";
import StatCard from "../components/StatCard";
import type { PageKey } from "../components/NavRail";
import { cn } from "../lib/cn";
import {
  formatBytes,
  formatDuration,
  formatLatency,
  formatRate,
  POLICY_LABEL,
} from "../lib/format";
import { useAppStore } from "../store/useAppStore";

function QuickLink({
  icon,
  label,
  value,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group flex flex-1 items-center gap-2.5 rounded-xl border border-hairline bg-white/[0.02] px-3 py-2.5 text-left transition-all hover:-translate-y-px hover:bg-white/[0.05]"
    >
      <span className="grid h-7 w-7 shrink-0 place-items-center rounded-lg bg-white/[0.04] text-ink-muted transition-colors group-hover:text-accent">
        {icon}
      </span>
      <span className="min-w-0">
        <span className="block text-[10px] text-ink-faint">{label}</span>
        <span className="block truncate font-mono text-[11px] text-ink">{value}</span>
      </span>
      <span className="ml-auto text-ink-faint/50 transition-colors group-hover:text-accent">›</span>
    </button>
  );
}

/** WebView 里 clipboard 可能不可用，保留一个不依赖权限的兜底。 */
async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    return ok;
  }
}

export default function HomePage({ onNavigate }: { onNavigate: (page: PageKey) => void }) {
  const status = useAppStore((s) => s.status);
  const config = useAppStore((s) => s.config);
  const rules = useAppStore((s) => s.rules);
  const traffic = useAppStore((s) => s.traffic);
  const active = useAppStore((s) => s.active);
  const history = useAppStore((s) => s.history);
  const busy = useAppStore((s) => s.busy);
  const toggle = useAppStore((s) => s.toggle);
  const toast = useAppStore((s) => s.toast);
  const activeRef = useRef<HTMLDivElement>(null);
  const copiedTimer = useRef<number | null>(null);
  const [copied, setCopied] = useState(false);

  const selected = useMemo(
    () => config?.nodes.find((n) => n.id === config.selectedNode) ?? null,
    [config]
  );
  const running = Boolean(status?.running);
  const localAddress = `127.0.0.1:${status?.localPort ?? config?.listenPort ?? 8888}`;
  const up = formatRate(traffic.upRate);
  const down = formatRate(traffic.downRate);
  const scrollToActive = () =>
    activeRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });

  useEffect(
    () => () => {
      if (copiedTimer.current !== null) window.clearTimeout(copiedTimer.current);
    },
    []
  );

  const copyLocalAddress = async () => {
    if (!(await copyText(localAddress))) {
      toast("复制失败，请手动选择地址", "error");
      return;
    }
    setCopied(true);
    toast("本地代理地址已复制", "ok");
    if (copiedTimer.current !== null) window.clearTimeout(copiedTimer.current);
    copiedTimer.current = window.setTimeout(() => setCopied(false), 1600);
  };

  return (
    <div className="animate-fade-up px-6 py-5">
      <div className="card relative flex items-center gap-6 overflow-hidden px-6 py-5 max-[860px]:flex-col max-[860px]:items-start">
        <div
          className={cn(
            "pointer-events-none absolute -left-24 -top-24 h-64 w-64 rounded-full blur-[70px] transition-colors duration-700",
            running ? "bg-ok/15" : "bg-accent/10"
          )}
          aria-hidden
        />

        <div className="relative z-10 shrink-0 max-[860px]:self-center">
          {running && (
            <>
              <span className="absolute inset-[-14px] rounded-full border border-ok/20 animate-breathe" aria-hidden />
              <span
                className="absolute inset-[-6px] rounded-full bg-[conic-gradient(from_0deg,rgba(48,209,88,0.0),rgba(48,209,88,0.55),rgba(100,210,255,0.45),rgba(48,209,88,0.0))] blur-[6px] animate-spin-slow"
                aria-hidden
              />
            </>
          )}
          <button
            type="button"
            onClick={() => void toggle()}
            disabled={busy || !selected}
            aria-label={running ? "关闭代理" : "开启代理"}
            className={cn(
              "group relative grid h-[124px] w-[124px] place-items-center rounded-full border transition-all duration-300 max-[860px]:h-[96px] max-[860px]:w-[96px]",
              running
                ? "border-ok/40 bg-[radial-gradient(circle_at_32%_26%,#1D3A2C,#0E1A15_72%)] text-ok shadow-[0_0_60px_rgba(48,209,88,0.28)]"
                : "border-white/[0.08] bg-[radial-gradient(circle_at_30%_25%,#232A34,#12161B_70%)] text-ink-faint hover:border-white/15 hover:text-ink-muted",
              busy && "cursor-wait",
              !selected && "cursor-not-allowed opacity-70"
            )}
          >
            {busy ? (
              <Loader2 size={30} className="animate-spin" />
            ) : (
              <Power size={34} strokeWidth={1.5} />
            )}
            <span
              className={cn(
                "absolute bottom-5 text-[10px] tracking-[0.22em]",
                running ? "text-ok/80" : "text-ink-faint/80"
              )}
            >
              {running ? "ON" : "OFF"}
            </span>
          </button>
        </div>

        <div className="relative z-10 min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <div className="flex min-w-0 flex-wrap items-center gap-2">
              <p className="text-sub text-ink">
                {selected ? selected.name : "尚未选择节点"}
              </p>
              <button
                type="button"
                onClick={() => void copyLocalAddress()}
                title={`复制 ${localAddress}`}
                aria-label={`复制本地代理地址 ${localAddress}`}
                className="group inline-flex shrink-0 items-center gap-1.5 rounded-md border border-hairline bg-white/[0.03] px-2 py-1 font-mono text-[11px] text-ink-muted transition-colors hover:bg-white/[0.06] hover:text-ink"
              >
                {localAddress}
                {copied ? (
                  <Check size={11} className="text-ok" />
                ) : (
                  <Copy size={11} className="text-ink-faint transition-colors group-hover:text-ink-muted" />
                )}
              </button>
              {selected && (
                <span className="rounded-md border border-hairline bg-white/[0.03] px-1.5 py-[2px] font-mono text-[10px] text-ink-faint">
                  {formatLatency(selected.latencyMs)}
                </span>
              )}
              <span className="hidden text-[10px] text-ink-faint min-[860px]:inline">
                {running
                  ? status?.systemProxy
                    ? "系统代理已接管"
                    : "仅本地端口监听"
                  : "系统代理未开启"}
              </span>
            </div>
            <span
              className={cn(
                "ml-auto flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-1 text-[10px]",
                running
                  ? "border-ok/25 bg-ok/10 text-ok"
                  : "border-hairline bg-white/[0.03] text-ink-faint"
              )}
            >
              <span
                className={cn(
                  "h-1.5 w-1.5 rounded-full",
                  running ? "bg-ok shadow-[0_0_8px_rgba(48,209,88,0.9)]" : "bg-ink-faint"
                )}
              />
              {running ? `运行中 ${formatDuration(status?.startedAt)}` : "引擎待机"}
            </span>
          </div>
          <p className="mt-1.5 text-[12px] leading-5 text-ink-muted">
            {selected
              ? running
                ? `流量经 ${selected.host}:${selected.port} 转发，规则命中后自动选择代理、直连或拒绝。`
                : `点击左侧按钮即可开始接管流量，当前节点 ${selected.host}:${selected.port}。`
              : "先在节点页粘贴一条 vmess:// 链接，或手动填写服务器信息。"}
          </p>
          <div className="mt-4 flex flex-wrap gap-2">
            <span className="rounded-lg border border-hairline bg-white/[0.03] px-2 py-1 text-[11px] text-ink-faint">
              默认策略 · {POLICY_LABEL[status?.defaultPolicy ?? "proxy"]}
            </span>
            <span className="rounded-lg border border-hairline bg-white/[0.03] px-2 py-1 text-[11px] text-ink-faint">
              规则 · {(rules?.total ?? 0).toLocaleString()} 条
            </span>
            <span className="rounded-lg border border-hairline bg-white/[0.03] px-2 py-1 text-[11px] text-ink-faint">
              订阅 · {rules?.remote ? (rules.remote / 1000).toFixed(1) + "k" : 0} 条生效
            </span>
          </div>
          {selected && (
            <div className="mt-4 flex flex-wrap gap-2">
              <button
                type="button"
                onClick={() => onNavigate("nodes")}
                className="inline-flex h-7 items-center gap-1.5 rounded-lg border border-hairline bg-white/[0.04] px-2.5 text-[11px] text-ink-muted transition-colors hover:bg-white/[0.08] hover:text-ink"
              >
                <Server size={12} /> 切换节点
              </button>
              <button
                type="button"
                onClick={() => onNavigate("rules")}
                className="inline-flex h-7 items-center gap-1.5 rounded-lg border border-hairline bg-white/[0.04] px-2.5 text-[11px] text-ink-muted transition-colors hover:bg-white/[0.08] hover:text-ink"
              >
                <ListFilter size={12} /> 查看规则
              </button>
            </div>
          )}
        </div>
      </div>

      <div className="mt-3 flex gap-3 max-[860px]:flex-col">
        <StatCard
          label="上传"
          value={up.value}
          unit={up.unit}
          icon={<ArrowUp size={16} />}
          tone="up"
          hint={`累计 ${formatBytes(traffic.upTotal)}`}
        />
        <StatCard
          label="下载"
          value={down.value}
          unit={down.unit}
          icon={<ArrowDown size={16} />}
          tone="down"
          hint={`累计 ${formatBytes(traffic.downTotal)}`}
        />
        <StatCard
          label="活动连接"
          value={String(active.length)}
          unit="个"
          icon={<Radio size={16} />}
          hint={`点击查看 · 本次会话 ${traffic.totalConnections} 次`}
          onClick={scrollToActive}
        />
      </div>

      <section className="card mt-3 px-4 pb-2 pt-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <h2 className="text-[12px] font-medium text-ink-muted">实时流量</h2>
            <span className="text-[10px] text-ink-faint">近 60 秒</span>
          </div>
          <div className="flex items-center gap-3 text-[10px]">
            <span className="flex items-center gap-1 text-info">
              <span className="h-1.5 w-3 rounded-full bg-info" /> 下载
            </span>
            <span className="flex items-center gap-1 text-ok">
              <span className="h-1.5 w-3 rounded-full bg-ok" /> 上传
            </span>
          </div>
        </div>
        <div className="relative">
          <TrafficChart data={history} dimmed={!running} height={116} />
          {!running && (
            <p className="pointer-events-none absolute inset-0 grid place-items-center text-[11px] text-ink-faint">
              开启代理后开始记录流量
            </p>
          )}
        </div>
      </section>

      <div className="mt-3 flex gap-3 max-[860px]:flex-col">
        <QuickLink
          icon={<ShieldCheck size={14} />}
          label="本地端口"
          value={`${config?.allowLan ? "0.0.0.0" : "127.0.0.1"}:${status?.localPort ?? 8888}`}
          onClick={() => onNavigate("settings")}
        />
        <QuickLink
          icon={<ListFilter size={14} />}
          label="规则来源"
          value={`自定义 ${rules?.custom ?? 0} + 远程 ${(rules?.remote ?? 0).toLocaleString()}`}
          onClick={() => onNavigate("rules")}
        />
        <QuickLink
          icon={<CircleSlash size={14} />}
          label="兜底策略"
          value={POLICY_LABEL[status?.defaultPolicy ?? "proxy"]}
          onClick={() => onNavigate("settings")}
        />
      </div>

      <div ref={activeRef} className="scroll-mt-3">
        <ActiveConnections entries={active} />
      </div>
    </div>
  );
}
