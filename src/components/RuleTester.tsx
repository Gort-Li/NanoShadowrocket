import { CornerDownLeft, FlaskConical, Loader2 } from "lucide-react";
import { POLICY_LABEL, policyTone } from "../lib/format";
import { useRuleStore } from "../store/useRuleStore";

export default function RuleTester() {
  const testInput = useRuleStore((s) => s.testInput);
  const setTestInput = useRuleStore((s) => s.setTestInput);
  const runTest = useRuleStore((s) => s.runTest);
  const testResult = useRuleStore((s) => s.testResult);
  const testing = useRuleStore((s) => s.testing);

  return (
    <section className="card overflow-hidden">
      <header className="flex items-center justify-between border-b border-hairline px-4 py-3">
        <div className="flex items-center gap-2">
          <FlaskConical size={14} className="text-ink-muted" />
          <h2 className="text-[13px] font-medium text-ink">规则测试</h2>
        </div>
        <span className="text-[11px] text-ink-faint">回车即可测试，不会发起真实连接</span>
      </header>

      <div className="px-4 py-3">
        <div className="flex items-center gap-2">
          <div className="flex h-9 min-w-0 flex-1 items-center gap-2 rounded-[10px] border border-hairline bg-black/25 px-3 transition-colors focus-within:border-accent/50">
            <input
              value={testInput}
              onChange={(e) => setTestInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void runTest();
              }}
              placeholder="输入域名或 URL，例如 api.github.com"
              spellCheck={false}
              className="h-full min-w-0 flex-1 border-0 bg-transparent font-mono text-[11px] text-ink shadow-none outline-none placeholder:text-ink-faint/70"
            />
            <span className="flex shrink-0 items-center gap-1 text-[10px] text-ink-faint">
              <CornerDownLeft size={10} /> Enter
            </span>
          </div>
          <button
            type="button"
            onClick={() => void runTest()}
            disabled={testing || !testInput.trim()}
            className="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-[10px] bg-accent px-3 text-[12px] font-medium text-white transition-colors hover:bg-[#2A96FF] disabled:cursor-not-allowed disabled:opacity-50"
          >
            {testing ? <Loader2 size={13} className="animate-spin" /> : null}
            测试
          </button>
        </div>

        {testResult && (
          <div className="mt-3 rounded-xl border border-hairline bg-white/[0.025] px-3.5 py-3">
            <div className="flex flex-wrap items-center gap-2">
              <span
                className={`rounded-md border px-1.5 py-[1px] text-[10px] ${policyTone(testResult.policy)}`}
              >
                {POLICY_LABEL[testResult.policy]}
              </span>
              <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-ink">
                {testResult.input}
              </span>
              <span className="text-[10px] text-ink-faint">
                {testResult.matched ? "命中规则" : "未命中"}
              </span>
            </div>

            {testResult.matched ? (
              <div className="mt-2 space-y-1 text-[11px] leading-5">
                <p className="font-mono text-ink-muted">{testResult.rule}</p>
                <p className="text-ink-faint">
                  来源 {testResult.source ?? "—"}
                  {testResult.ruleLine ? ` · 第 ${testResult.ruleLine} 行` : ""}
                  {testResult.resolvedIp ? ` · 解析到 ${testResult.resolvedIp}` : ""}
                </p>
              </div>
            ) : (
              <p className="mt-2 text-[11px] leading-5 text-ink-muted">
                没有任何规则命中，将走默认策略「{POLICY_LABEL[testResult.defaultPolicy]}」。
                {testResult.resolvedIp ? ` 已解析到 ${testResult.resolvedIp}。` : ""}
              </p>
            )}
            <p className="mt-1.5 text-[10px] text-ink-faint/80">
              当前参与匹配的规则共 {testResult.totalRules.toLocaleString()} 条（含内置直连规则）
            </p>
          </div>
        )}
      </div>
    </section>
  );
}
