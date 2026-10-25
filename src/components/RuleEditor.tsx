import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  Pencil,
  Plus,
  RotateCcw,
  Save,
  Search,
  Trash2,
} from "lucide-react";
import { useMemo, useState } from "react";
import { cn } from "../lib/cn";
import {
  formatCustomRule,
  parseCustomRules,
  serializeCustomRules,
  upsertCustomRule,
  type CustomRule,
} from "../lib/customRules";
import { POLICY_LABEL, policyTone } from "../lib/format";
import { useRuleStore } from "../store/useRuleStore";
import Button from "./Button";
import ConfirmDialog from "./ConfirmDialog";
import RuleSheet from "./RuleSheet";

function IconButton({
  label,
  disabled,
  danger,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  danger?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "grid h-7 w-7 place-items-center rounded-md text-ink-faint transition-colors",
        danger ? "hover:bg-danger/10 hover:text-danger" : "hover:bg-white/[0.07] hover:text-ink",
        disabled && "cursor-not-allowed opacity-25 hover:bg-transparent hover:text-ink-faint"
      )}
    >
      {children}
    </button>
  );
}

export default function RuleEditor() {
  const custom = useRuleStore((s) => s.custom);
  const summary = useRuleStore((s) => s.summary);
  const saving = useRuleStore((s) => s.saving);
  const setCustom = useRuleStore((s) => s.setCustom);
  const resetCustom = useRuleStore((s) => s.resetCustom);
  const saveCustom = useRuleStore((s) => s.saveCustom);

  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<{ index: number | null; rule: CustomRule } | null>(
    null
  );
  const [deleting, setDeleting] = useState<number | null>(null);

  const parsed = useMemo(() => parseCustomRules(custom), [custom]);
  const filter = query.trim().toLowerCase();
  const visible = parsed.rules
    .map((rule, index) => ({ rule, index }))
    .filter(({ rule }) => {
      if (!filter) return true;
      return [rule.kind, rule.value, rule.policy, POLICY_LABEL[rule.policy]].some((value) =>
        value.toLowerCase().includes(filter)
      );
    });
  const canReorder = !filter;

  const updateRules = (rules: CustomRule[], unsupported = parsed.unsupported) => {
    setCustom(serializeCustomRules(rules, unsupported));
  };

  const move = (index: number, delta: number) => {
    const target = index + delta;
    if (target < 0 || target >= parsed.rules.length) return;
    const next = [...parsed.rules];
    [next[index], next[target]] = [next[target], next[index]];
    updateRules(next);
  };

  const saveRule = (rule: CustomRule) => {
    if (!editing) return;
    if (editing.index === null) {
      updateRules(upsertCustomRule(parsed.rules, rule));
    } else {
      updateRules(parsed.rules.map((item, index) => (index === editing.index ? rule : item)));
    }
    setEditing(null);
  };

  const deleteRule = () => {
    if (deleting === null) return;
    updateRules(parsed.rules.filter((_, index) => index !== deleting));
    setDeleting(null);
  };

  const clearUnsupported = () => {
    updateRules(parsed.rules, []);
  };

  return (
    <section className="card overflow-hidden">
      <header className="flex flex-wrap items-center justify-between gap-3 border-b border-hairline px-4 py-3">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-[13px] font-medium text-ink">自定义规则</h2>
          <span className="rounded-md border border-accent/25 bg-accent-soft px-1.5 py-[1px] text-[10px] text-accent">
            优先级最高
          </span>
          <span className="font-mono text-[10px] text-ink-faint">{parsed.rules.length} 条</span>
          {summary && (
            <span className="text-[10px] text-ink-faint">已生效 {summary.custom} 条</span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" icon={<RotateCcw size={13} />} onClick={resetCustom}>
            恢复默认
          </Button>
          <Button
            variant="primary"
            icon={<Save size={13} />}
            loading={saving}
            onClick={() => void saveCustom()}
          >
            保存
          </Button>
        </div>
      </header>

      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-hairline px-4 py-2">
        <div className="flex items-center gap-2">
          <Button
            variant="primary"
            icon={<Plus size={13} />}
            onClick={() =>
              setEditing({
                index: null,
                rule: { kind: "DOMAIN-SUFFIX", value: "", policy: "proxy", noResolve: false },
              })
            }
          >
            添加规则
          </Button>
          <span className="text-[10px] text-ink-faint">新规则会加入顶部，按列表顺序匹配</span>
        </div>
        <label className="flex h-8 w-[190px] items-center gap-1.5 rounded-lg border border-hairline bg-black/20 px-2">
          <Search size={12} className="shrink-0 text-ink-faint" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="搜索类型、域名或策略"
            className="min-w-0 flex-1 bg-transparent text-[11px] text-ink outline-none placeholder:text-ink-faint"
          />
        </label>
      </div>

      {parsed.unsupported.length > 0 && (
        <div className="mx-4 mt-3 flex items-start gap-2 rounded-lg border border-warn/25 bg-warn/10 px-3 py-2 text-[11px] leading-5 text-warn">
          <AlertTriangle size={13} className="mt-0.5 shrink-0" />
          <span className="flex-1">
            有 {parsed.unsupported.length} 行无法在表单中编辑，保存时会原样保留。
          </span>
          <button
            type="button"
            onClick={clearUnsupported}
            className="shrink-0 text-[10px] text-warn/80 underline-offset-2 hover:underline"
          >
            清除
          </button>
        </div>
      )}

      {parsed.rules.length === 0 ? (
        <div className="px-4 py-10 text-center">
          <p className="text-[12px] text-ink-muted">还没有自定义规则</p>
          <p className="mt-1 text-[11px] text-ink-faint">
            添加后永远优先于远程订阅，保存后立即生效
          </p>
          <Button
            variant="primary"
            icon={<Plus size={13} />}
            className="mt-4"
            onClick={() =>
              setEditing({
                index: null,
                rule: { kind: "DOMAIN-SUFFIX", value: "", policy: "proxy", noResolve: false },
              })
            }
          >
            添加第一条规则
          </Button>
        </div>
      ) : visible.length === 0 ? (
        <p className="px-4 py-8 text-center text-[11px] text-ink-faint">没有匹配的规则</p>
      ) : (
        <div className="divide-y divide-white/[0.05]">
          {visible.map(({ rule, index }) => (
            <div
              key={`${rule.kind}-${rule.value}-${index}`}
              className="flex items-center gap-2 px-3 py-2 transition-colors hover:bg-white/[0.02]"
            >
              <span className="w-5 shrink-0 text-right font-mono text-[10px] text-ink-faint">
                {index + 1}
              </span>
              <span className="w-[108px] shrink-0 truncate rounded-md border border-hairline bg-white/[0.03] px-1.5 py-[1px] font-mono text-[10px] text-ink-muted">
                {rule.kind}
              </span>
              <span
                className="min-w-0 flex-1 truncate font-mono text-[11px] text-ink"
                title={formatCustomRule(rule)}
              >
                {rule.kind === "FINAL" ? "—" : rule.value}
              </span>
              {rule.noResolve && (
                <span className="shrink-0 rounded border border-hairline px-1 py-[1px] font-mono text-[9px] text-ink-faint">
                  no-resolve
                </span>
              )}
              <span
                className={cn(
                  "shrink-0 rounded-md border px-1.5 py-[1px] text-[10px]",
                  policyTone(rule.policy)
                )}
              >
                {POLICY_LABEL[rule.policy]}
              </span>
              <div className="flex shrink-0 items-center gap-0.5">
                <IconButton
                  label="上移"
                  disabled={!canReorder || index === 0}
                  onClick={() => move(index, -1)}
                >
                  <ArrowUp size={12} />
                </IconButton>
                <IconButton
                  label="下移"
                  disabled={!canReorder || index === parsed.rules.length - 1}
                  onClick={() => move(index, 1)}
                >
                  <ArrowDown size={12} />
                </IconButton>
                <IconButton label="编辑" onClick={() => setEditing({ index, rule })}>
                  <Pencil size={12} />
                </IconButton>
                <IconButton label="删除" danger onClick={() => setDeleting(index)}>
                  <Trash2 size={12} />
                </IconButton>
              </div>
            </div>
          ))}
        </div>
      )}

      <footer className="border-t border-hairline px-4 py-2.5 text-[10px] text-ink-faint">
        规则从上到下匹配，命中即止；点击「保存」后生效，表单不保留注释。
      </footer>

      <RuleSheet
        open={editing !== null}
        title={editing?.index == null ? "添加自定义规则" : "编辑自定义规则"}
        submitLabel={editing?.index == null ? "加入列表" : "应用修改"}
        rule={editing?.rule ?? null}
        onClose={() => setEditing(null)}
        onSave={saveRule}
      />

      <ConfirmDialog
        open={deleting !== null}
        title="删除自定义规则"
        description={
          deleting !== null && parsed.rules[deleting]
            ? `确定删除「${formatCustomRule(parsed.rules[deleting])}」吗？保存后立即生效。`
            : "确定删除这条规则吗？"
        }
        confirmText="删除"
        onConfirm={deleteRule}
        onCancel={() => setDeleting(null)}
      />
    </section>
  );
}
