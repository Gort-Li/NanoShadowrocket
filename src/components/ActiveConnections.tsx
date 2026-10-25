import {
  Activity,
  Ban,
  ChevronDown,
  ChevronRight,
  Globe2,
  Route,
  ShieldCheck,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { groupActiveConnections } from "../lib/active";
import { cn } from "../lib/cn";
import { ruleForHost } from "../lib/customRules";
import { POLICY_LABEL, policyTone } from "../lib/format";
import type { ActiveConnection, Policy } from "../lib/types";
import { useAppStore } from "../store/useAppStore";
import { useRuleStore } from "../store/useRuleStore";

const ACTIONS: Array<{
  policy: Policy;
  label: string;
  icon: React.ReactNode;
  className: string;
}> = [
  {
    policy: "proxy",
    label: "代理",
    icon: <ShieldCheck size={10} />,
    className: "border-accent/30 bg-accent-soft text-accent hover:bg-accent/20",
  },
  {
    policy: "direct",
    label: "直连",
    icon: <Route size={10} />,
    className: "border-white/10 bg-white/[0.04] text-ink-muted hover:bg-white/[0.08]",
  },
  {
    policy: "reject",
    label: "拒绝",
    icon: <Ban size={10} />,
    className: "border-danger/30 bg-danger/10 text-danger hover:bg-danger/20",
  },
];

function PolicyBadge({ policy, mixed }: { policy: Policy; mixed: boolean }) {
  return (
    <span
      className={cn(
        "shrink-0 rounded-md border px-1.5 py-[1px] text-[10px]",
        policyTone(policy)
      )}
    >
      {POLICY_LABEL[policy]}
      {mixed && <span className="ml-1 text-ink-faint">混合</span>}
    </span>
  );
}

function ProtocolBadge({ protocols }: { protocols: Array<"tcp" | "udp"> }) {
  const label = protocols.length > 1 ? "TCP/UDP" : protocols[0]?.toUpperCase() ?? "TCP";
  return (
    <span className="shrink-0 rounded border border-white/10 bg-white/[0.03] px-1 py-[1px] font-mono text-[9px] text-ink-faint">
      {label}
    </span>
  );
}

function QuickActions({
  host,
  scope,
  disabled,
}: {
  host: string;
  scope: "exact" | "suffix";
  disabled: boolean;
}) {
  const applyQuickRule = useRuleStore((s) => s.applyQuickRule);
  return (
    <div className="flex shrink-0 items-center gap-1">
      {ACTIONS.map((action) => (
        <button
          key={action.policy}
          type="button"
          disabled={disabled}
          title={`将 ${host} 设为${action.label}（一键写入自定义规则）`}
          aria-label={`将 ${host} 设为${action.label}`}
          onClick={() => void applyQuickRule(ruleForHost(host, action.policy, scope))}
          className={cn(
            "inline-flex items-center gap-0.5 rounded-md border px-1.5 py-[2px] text-[10px] transition-colors",
            action.className,
            disabled && "cursor-not-allowed opacity-40"
          )}
        >
          {action.icon}
          {action.label}
        </button>
      ))}
    </div>
  );
}

export default function ActiveConnections({ entries }: { entries: ActiveConnection[] }) {
  const running = useAppStore((s) => s.status?.running ?? false);
  const saving = useRuleStore((s) => s.saving);
  const groups = useMemo(() => groupActiveConnections(entries), [entries]);
  const [expanded, setExpanded] = useState<string | null>(null);

  useEffect(() => {
    if (expanded && !groups.some((group) => group.domain === expanded)) {
      setExpanded(null);
    }
  }, [expanded, groups]);

  return (
    <section id="active-connections" className="mt-5">
      <div className="mb-2 flex flex-wrap items-center gap-2 px-1">
        <Activity size={14} className="text-ink-muted" />
        <h2 className="text-[13px] font-medium text-ink">当前活动链接</h2>
        <span className="rounded-full border border-hairline bg-white/[0.03] px-2 py-0.5 font-mono text-[10px] text-ink-muted">
          {entries.length}
        </span>
        <span className="text-[10px] text-ink-faint">按主域名聚合 · 代理优先</span>
      </div>

      {groups.length === 0 ? (
        <div className="card px-4 py-8 text-center">
          <p className="text-[12px] text-ink-muted">当前没有活动链接</p>
          <p className="mt-1 text-[11px] text-ink-faint">
            {running
              ? "有程序通过本机代理发起连接后，会实时出现在这里"
              : "开启代理后，活跃的 TCP / UDP 目标会显示在这里"}
          </p>
        </div>
      ) : (
        <div className="card overflow-hidden">
          {groups.map((group) => {
            const isExpanded = expanded === group.domain;
            return (
              <div key={group.domain} className="border-b border-hairline last:border-b-0">
                <div className="flex items-center gap-2 px-3 py-2.5 transition-colors hover:bg-white/[0.03]">
                  <button
                    type="button"
                    onClick={() => setExpanded(isExpanded ? null : group.domain)}
                    aria-expanded={isExpanded}
                    className="flex min-w-0 flex-1 items-center gap-2 text-left"
                  >
                    {isExpanded ? (
                      <ChevronDown size={14} className="shrink-0 text-ink-faint" />
                    ) : (
                      <ChevronRight size={14} className="shrink-0 text-ink-faint" />
                    )}
                    <Globe2 size={13} className="shrink-0 text-ink-muted" />
                    <span className="truncate font-mono text-[12px] text-ink">{group.domain}</span>
                    <PolicyBadge policy={group.policy} mixed={group.mixed} />
                    <span className="shrink-0 text-[10px] text-ink-faint">
                      {group.count} 个连接
                    </span>
                    <span className="hidden shrink-0 text-[10px] text-ink-faint min-[980px]:inline">
                      {group.hosts.length} 个主机
                    </span>
                  </button>
                  <QuickActions host={group.domain} scope="suffix" disabled={saving} />
                </div>

                {isExpanded && (
                  <div className="border-t border-white/[0.04] bg-black/10 py-1 pl-7 pr-2">
                    {group.hosts.map((host) => (
                      <div
                        key={host.host}
                        className="flex flex-wrap items-center gap-2 rounded-lg px-2 py-1.5 transition-colors hover:bg-white/[0.03]"
                      >
                        <div className="flex min-w-0 flex-1 items-center gap-1.5 overflow-hidden">
                          <span className="h-1 w-1 shrink-0 rounded-full bg-ink-faint/60" />
                          <span
                            className="min-w-0 truncate font-mono text-[11px] text-ink-muted"
                            title={host.rule ?? "默认策略"}
                          >
                            {host.host}
                          </span>
                          <span className="shrink-0 font-mono text-[10px] text-ink-faint">
                            {host.ports.map((port) => `:${port}`).join(" / ")}
                          </span>
                          <ProtocolBadge protocols={host.protocols} />
                          <PolicyBadge policy={host.policy} mixed={host.mixed} />
                          {host.count > 1 && (
                            <span className="shrink-0 font-mono text-[10px] text-ink-faint">
                              ×{host.count}
                            </span>
                          )}
                        </div>
                        <QuickActions host={host.host} scope="exact" disabled={saving} />
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
