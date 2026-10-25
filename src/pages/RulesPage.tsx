import { Plus, RefreshCw, Rss } from "lucide-react";
import { useState } from "react";
import Button from "../components/Button";
import ConfirmDialog from "../components/ConfirmDialog";
import EmptyState from "../components/EmptyState";
import PageHeader from "../components/PageHeader";
import RemoteRuleCard from "../components/RemoteRuleCard";
import RuleEditor from "../components/RuleEditor";
import RuleTester from "../components/RuleTester";
import Sheet from "../components/Sheet";
import type { RemoteRule } from "../lib/types";
import { useRuleStore } from "../store/useRuleStore";

export default function RulesPage() {
  const remotes = useRuleStore((s) => s.remotes);
  const summary = useRuleStore((s) => s.summary);
  const addRemote = useRuleStore((s) => s.addRemote);
  const removeRemote = useRuleStore((s) => s.removeRemote);
  const updateAll = useRuleStore((s) => s.updateAll);
  const reorder = useRuleStore((s) => s.reorder);

  const [sheetOpen, setSheetOpen] = useState(false);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [intervalHours, setIntervalHours] = useState(12);
  const [submitting, setSubmitting] = useState(false);
  const [updatingAll, setUpdatingAll] = useState(false);
  const [deleting, setDeleting] = useState<RemoteRule | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);

  const openSheet = () => {
    setName("");
    setUrl("");
    setIntervalHours(12);
    setSheetOpen(true);
  };

  const submit = async () => {
    setSubmitting(true);
    const ok = await addRemote(name, url, intervalHours);
    setSubmitting(false);
    if (ok) setSheetOpen(false);
  };

  const handleDrop = async (targetId: string) => {
    if (!dragId || dragId === targetId) {
      setDragId(null);
      return;
    }
    const ids = remotes.map((r) => r.id).filter((id) => id !== dragId);
    const index = ids.indexOf(targetId);
    ids.splice(index < 0 ? ids.length : index, 0, dragId);
    setDragId(null);
    await reorder(ids);
  };

  return (
    <div className="animate-fade-up px-6 py-5">
      <PageHeader
        title="规则"
        description="自定义规则永远优先，远程订阅按下方顺序依次匹配"
        actions={
          <>
            <Button
              icon={<RefreshCw size={13} />}
              loading={updatingAll}
              disabled={remotes.length === 0}
              onClick={async () => {
                setUpdatingAll(true);
                await updateAll();
                setUpdatingAll(false);
              }}
            >
              更新全部
            </Button>
            <Button variant="primary" icon={<Plus size={13} />} onClick={openSheet}>
              添加订阅
            </Button>
          </>
        }
      />

      <div className="mb-3 flex flex-wrap items-center gap-2 text-[11px] text-ink-faint">
        <span className="rounded-lg border border-hairline bg-white/[0.03] px-2 py-1">
          共 {(summary?.total ?? 0).toLocaleString()} 条
        </span>
        <span className="rounded-lg border border-hairline bg-white/[0.03] px-2 py-1">
          自定义 {summary?.custom ?? 0}
        </span>
        <span className="rounded-lg border border-hairline bg-white/[0.03] px-2 py-1">
          远程 {(summary?.remote ?? 0).toLocaleString()}
        </span>
        {(summary?.ignored ?? 0) > 0 && (
          <span className="rounded-lg border border-warn/25 bg-warn/10 px-2 py-1 text-warn">
            已忽略 {summary?.ignored} 条不支持的规则
          </span>
        )}
      </div>

      <div className="flex flex-col gap-3">
        <RuleEditor />

        <section>
          <div className="mb-2 flex items-center justify-between px-1">
            <div className="flex items-center gap-2">
              <Rss size={13} className="text-ink-muted" />
              <h2 className="text-[12px] font-medium text-ink-muted">远程规则订阅</h2>
              <span className="text-[10px] text-ink-faint">拖动左侧手柄调整优先级</span>
            </div>
            <span className="text-[10px] text-ink-faint">越靠上优先级越高</span>
          </div>

          {remotes.length === 0 ? (
            <EmptyState
              icon={Rss}
              title="还没有远程订阅"
              description="粘贴 Shadowrocket 格式的 .conf 规则地址，例如 Johnshall 的分流规则合集，保存后会自动下载并按间隔刷新。"
              hint="https://example.com/lazy.conf"
            />
          ) : (
            <div className="flex flex-col gap-2">
              {remotes.map((remote) => (
                <RemoteRuleCard
                  key={remote.id}
                  remote={remote}
                  dragging={dragId === remote.id}
                  onDelete={() => setDeleting(remote)}
                  onDragStart={() => setDragId(remote.id)}
                  onDragOver={(event) => event.preventDefault()}
                  onDrop={() => void handleDrop(remote.id)}
                  onDragEnd={() => setDragId(null)}
                />
              ))}
            </div>
          )}
        </section>

        <RuleTester />
      </div>

      <Sheet
        open={sheetOpen}
        onClose={() => setSheetOpen(false)}
        title="添加远程订阅"
        description="支持 Shadowrocket .conf 格式；只解析 [Rule] 段，其它段自动忽略"
        footer={
          <>
            <Button variant="ghost" onClick={() => setSheetOpen(false)}>
              取消
            </Button>
            <Button
              variant="primary"
              loading={submitting}
              disabled={!url.trim()}
              onClick={() => void submit()}
            >
              下载并保存
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <label className="block">
            <span className="field-label">名称（可选）</span>
            <input
              className="field"
              value={name}
              placeholder="Johnshall 分流规则"
              onChange={(e) => setName(e.target.value)}
            />
          </label>

          <label className="block">
            <span className="field-label">规则地址</span>
            <textarea
              className="field-area h-[92px] font-mono text-[11px]"
              value={url}
              spellCheck={false}
              placeholder="https://raw.githubusercontent.com/.../lazy.conf"
              onChange={(e) => setUrl(e.target.value)}
            />
          </label>

          <label className="block">
            <span className="field-label">自动更新间隔</span>
            <select
              className="field"
              value={intervalHours}
              onChange={(e) => setIntervalHours(Number(e.target.value))}
            >
              {[0, 6, 12, 24, 48].map((hours) => (
                <option key={hours} value={hours}>
                  {hours === 0 ? "关闭自动更新" : `每 ${hours} 小时`}
                </option>
              ))}
            </select>
          </label>

          <div className="rounded-xl border border-hairline bg-white/[0.025] px-3 py-2.5 text-[11px] leading-5 text-ink-faint">
            保存后会立即下载一次；若服务器支持 ETag / Last-Modified，后续刷新会走 304
            短路，不再重复下载正文。
          </div>
        </div>
      </Sheet>

      <ConfirmDialog
        open={Boolean(deleting)}
        title={`删除订阅「${deleting?.name ?? ""}」`}
        description="删除后缓存的规则文件也会一并清理，相关域名将回落到后面的订阅或默认策略。"
        confirmText="删除"
        onCancel={() => setDeleting(null)}
        onConfirm={() => {
          if (deleting) void removeRemote(deleting.id);
          setDeleting(null);
        }}
      />
    </div>
  );
}
