import { Info } from "lucide-react";
import { useEffect, useState } from "react";
import { cn } from "../lib/cn";
import {
  formatCustomRule,
  validateCustomRule,
  type CustomRule,
  type CustomRuleKind,
} from "../lib/customRules";
import type { Policy } from "../lib/types";
import Button from "./Button";
import Sheet from "./Sheet";
import Switch from "./Switch";

const KINDS: Array<{ value: CustomRuleKind; label: string; hint: string }> = [
  { value: "DOMAIN-SUFFIX", label: "域名后缀", hint: "匹配该域名及其所有子域名" },
  { value: "DOMAIN", label: "精确域名", hint: "只匹配这一个主机名" },
  { value: "DOMAIN-KEYWORD", label: "域名关键词", hint: "域名中包含该关键词即命中" },
  { value: "IP-CIDR", label: "IP 网段", hint: "支持 IPv4 / IPv6 CIDR" },
  { value: "GEOIP", label: "国家/地区", hint: "按 IP 所属国家匹配，例如 CN" },
  { value: "FINAL", label: "兜底规则", hint: "覆盖默认策略；自定义 FINAL 优先级最高" },
];

const POLICIES: Array<{ value: Policy; label: string; className: string }> = [
  { value: "proxy", label: "代理", className: "border-accent/40 bg-accent-soft text-accent" },
  { value: "direct", label: "直连", className: "border-white/15 bg-white/[0.08] text-ink" },
  { value: "reject", label: "拒绝", className: "border-danger/40 bg-danger/10 text-danger" },
];

function placeholder(kind: CustomRuleKind): string {
  if (kind === "DOMAIN" || kind === "DOMAIN-SUFFIX") return "例如 api.github.com";
  if (kind === "DOMAIN-KEYWORD") return "例如 github";
  if (kind === "IP-CIDR") return "例如 203.0.113.0/24";
  if (kind === "GEOIP") return "例如 CN";
  return "";
}

export default function RuleSheet({
  open,
  title,
  submitLabel = "确定",
  rule,
  onClose,
  onSave,
}: {
  open: boolean;
  title: string;
  submitLabel?: string;
  rule: CustomRule | null;
  onClose: () => void;
  onSave: (rule: CustomRule) => void;
}) {
  const [draft, setDraft] = useState<CustomRule>(
    rule ?? { kind: "DOMAIN-SUFFIX", value: "", policy: "proxy", noResolve: false }
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setDraft(rule ?? { kind: "DOMAIN-SUFFIX", value: "", policy: "proxy", noResolve: false });
    setError(null);
  }, [open, rule]);

  const kind = KINDS.find((item) => item.value === draft.kind);
  const noResolveAvailable = draft.kind === "IP-CIDR" || draft.kind === "GEOIP";
  const preview = formatCustomRule({
    ...draft,
    value: draft.kind === "FINAL" || draft.value.trim() ? draft.value : "<匹配内容>",
  });

  const update = <K extends keyof CustomRule>(key: K, value: CustomRule[K]) => {
    setDraft((prev) => {
      const next = { ...prev, [key]: value };
      if (key === "kind") {
        if (value === "FINAL") {
          next.value = "";
          next.noResolve = false;
        } else if (value !== "IP-CIDR" && value !== "GEOIP") {
          next.noResolve = false;
        }
      }
      return next;
    });
    setError(null);
  };

  const submit = () => {
    const message = validateCustomRule(draft);
    if (message) {
      setError(message);
      return;
    }
    onSave({ ...draft, value: draft.value.trim() });
  };

  return (
    <Sheet
      open={open}
      onClose={onClose}
      title={title}
      description="先加入列表，点击页面右上角的「保存」后生效"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            取消
          </Button>
          <Button variant="primary" onClick={submit}>
            {submitLabel}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <label className="block">
          <span className="field-label">规则类型</span>
          <select
            className="field"
            value={draft.kind}
            onChange={(event) => update("kind", event.target.value as CustomRuleKind)}
          >
            {KINDS.map((item) => (
              <option key={item.value} value={item.value}>
                {item.label} · {item.value}
              </option>
            ))}
          </select>
          {kind && <span className="mt-1 block text-[10px] text-ink-faint">{kind.hint}</span>}
        </label>

        {draft.kind !== "FINAL" && (
          <label className="block">
            <span className="field-label">匹配内容</span>
            <input
              className="field font-mono text-[12px]"
              value={draft.value}
              placeholder={placeholder(draft.kind)}
              spellCheck={false}
              onChange={(event) => update("value", event.target.value)}
            />
          </label>
        )}

        <div>
          <span className="field-label">命中策略</span>
          <div className="flex gap-1 rounded-xl border border-hairline bg-black/20 p-1">
            {POLICIES.map((item) => (
              <button
                key={item.value}
                type="button"
                onClick={() => update("policy", item.value)}
                className={cn(
                  "h-8 flex-1 rounded-lg border text-[12px] transition-colors",
                  draft.policy === item.value
                    ? item.className
                    : "border-transparent text-ink-faint hover:text-ink-muted"
                )}
              >
                {item.label}
              </button>
            ))}
          </div>
        </div>

        {noResolveAvailable && (
          <div className="flex items-center justify-between gap-4 rounded-xl border border-hairline bg-white/[0.02] px-3 py-2.5">
            <div>
              <p className="text-[12px] text-ink">不触发 DNS 解析</p>
              <p className="mt-0.5 text-[10px] text-ink-faint">
                仅用目标 IP 判断，避免为 IP / GEOIP 规则额外解析域名
              </p>
            </div>
            <Switch
              checked={draft.noResolve}
              onChange={(next) => update("noResolve", next)}
              label="不触发 DNS 解析"
            />
          </div>
        )}

        <div className="rounded-xl border border-hairline bg-black/20 px-3 py-2.5">
          <p className="text-[10px] text-ink-faint">规则预览</p>
          <code className="mt-1 block break-all font-mono text-[11px] text-ink-muted">
            {preview}
          </code>
        </div>

        {draft.kind === "FINAL" && (
          <div className="flex items-start gap-2 rounded-lg border border-warn/25 bg-warn/10 px-3 py-2 text-[11px] leading-5 text-warn">
            <Info size={13} className="mt-0.5 shrink-0" />
            自定义 FINAL 会覆盖远程订阅与默认策略，建议只保留一条。
          </div>
        )}

        {error && (
          <div className="rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-[11px] text-danger">
            {error}
          </div>
        )}
      </div>
    </Sheet>
  );
}
