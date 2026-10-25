import { RotateCcw, Save } from "lucide-react";
import { useMemo, useRef } from "react";
import Button from "./Button";
import { useRuleStore } from "../store/useRuleStore";

const POLICY = /^\s*(proxy|direct|reject|allow|bypass|deny)\s*$/i;

function Line({ text }: { text: string }) {
  if (!text) return <span> </span>;
  const trimmed = text.trimStart();
  if (trimmed.startsWith("#") || trimmed.startsWith(";") || trimmed.startsWith("//")) {
    return <span className="text-ink-faint">{text}</span>;
  }
  const parts = text.split(",");
  return (
    <>
      {parts.map((part, index) => {
        const isPolicy = POLICY.test(part);
        const isNoResolve = /^\s*no-resolve\s*$/i.test(part);
        const color =
          index === 0
            ? "text-[#64D2FF]"
            : isPolicy
              ? /reject|deny/i.test(part)
                ? "text-danger"
                : /direct|bypass/i.test(part)
                  ? "text-ok"
                  : "text-accent"
              : isNoResolve
                ? "text-warn/80"
                : "text-ink";
        return (
          <span key={index}>
            {index > 0 && <span className="text-ink-faint/70">,</span>}
            <span className={color}>{part}</span>
          </span>
        );
      })}
    </>
  );
}

export default function RuleEditor() {
  const custom = useRuleStore((s) => s.custom);
  const setCustom = useRuleStore((s) => s.setCustom);
  const saveCustom = useRuleStore((s) => s.saveCustom);
  const resetCustom = useRuleStore((s) => s.resetCustom);
  const saving = useRuleStore((s) => s.saving);
  const summary = useRuleStore((s) => s.summary);

  const textRef = useRef<HTMLTextAreaElement>(null);
  const preRef = useRef<HTMLPreElement>(null);
  const gutterRef = useRef<HTMLDivElement>(null);

  const lines = useMemo(() => custom.split("\n"), [custom]);
  const editorHeight = Math.min(320, Math.max(152, lines.length * 20 + 18));

  return (
    <section className="card overflow-hidden">
      <header className="flex items-center justify-between gap-3 border-b border-hairline px-4 py-3">
        <div className="flex items-center gap-2">
          <h2 className="text-[13px] font-medium text-ink">自定义规则</h2>
          <span className="rounded-md border border-accent/30 bg-accent-soft px-1.5 py-[1px] text-[10px] text-accent">
            优先级最高
          </span>
          <span className="text-[11px] text-ink-faint">
            {summary?.custom ?? 0} 条生效
          </span>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" icon={<RotateCcw size={12} />} onClick={resetCustom}>
            恢复默认
          </Button>
          <Button
            variant="primary"
            icon={<Save size={12} />}
            loading={saving}
            onClick={() => void saveCustom()}
          >
            保存
          </Button>
        </div>
      </header>

      <div
        style={{ height: editorHeight }}
        className="relative m-3 flex overflow-hidden rounded-xl border border-hairline bg-black/25 transition-[height] duration-200"
      >
        <div
          ref={gutterRef}
          className="w-10 shrink-0 overflow-hidden border-r border-white/[0.05] bg-white/[0.015] py-2 text-right font-mono text-[11px] leading-[20px] text-ink-faint/50"
        >
          {lines.map((_, index) => (
            <div key={index} className="pr-2">
              {index + 1}
            </div>
          ))}
        </div>
        <div className="relative min-w-0 flex-1">
          <pre
            ref={preRef}
            aria-hidden
            className="pointer-events-none absolute inset-0 overflow-hidden whitespace-pre px-3 py-2 font-mono text-[11px] leading-[20px]"
          >
            {lines.map((line, index) => (
              <div key={index}>
                <Line text={line} />
              </div>
            ))}
          </pre>
          <textarea
            ref={textRef}
            value={custom}
            spellCheck={false}
            aria-label="自定义规则编辑器"
            onChange={(e) => setCustom(e.target.value)}
            onScroll={(e) => {
              const top = e.currentTarget.scrollTop;
              const left = e.currentTarget.scrollLeft;
              if (preRef.current) {
                preRef.current.scrollTop = top;
                preRef.current.scrollLeft = left;
              }
              if (gutterRef.current) gutterRef.current.scrollTop = top;
            }}
            className="absolute inset-0 h-full w-full resize-none overflow-auto whitespace-pre bg-transparent px-3 py-2 font-mono text-[11px] leading-[20px] text-transparent caret-accent outline-none selection:bg-accent/30"
          />
        </div>
      </div>

      <p className="px-4 pb-3 text-[11px] leading-5 text-ink-faint">
        语法与 Shadowrocket 一致，策略填 Proxy / Direct / Reject，IP 规则可追加 no-resolve。
      </p>
    </section>
  );
}
