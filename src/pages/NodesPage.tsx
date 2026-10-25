import { Plus, Server, Zap } from "lucide-react";
import { useState } from "react";
import Button from "../components/Button";
import ConfirmDialog from "../components/ConfirmDialog";
import EmptyState from "../components/EmptyState";
import NodeCard from "../components/NodeCard";
import NodeSheet from "../components/NodeSheet";
import PageHeader from "../components/PageHeader";
import { formatLatency, latencyTone } from "../lib/format";
import type { Node } from "../lib/types";
import { useAppStore } from "../store/useAppStore";

const DOT = {
  ok: "bg-ok shadow-[0_0_8px_rgba(48,209,88,0.8)]",
  warn: "bg-warn shadow-[0_0_8px_rgba(255,214,10,0.7)]",
  bad: "bg-danger shadow-[0_0_8px_rgba(255,69,58,0.7)]",
  none: "bg-ink-faint/60",
};

export default function NodesPage() {
  const config = useAppStore((s) => s.config);
  const status = useAppStore((s) => s.status);
  const busy = useAppStore((s) => s.busy);
  const selectNode = useAppStore((s) => s.selectNode);
  const deleteNode = useAppStore((s) => s.deleteNode);
  const testLatency = useAppStore((s) => s.testLatency);
  const testAll = useAppStore((s) => s.testAll);

  const [sheetOpen, setSheetOpen] = useState(false);
  const [editing, setEditing] = useState<Node | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<Node | null>(null);

  const nodes = config?.nodes ?? [];
  const selected = nodes.find((n) => n.id === config?.selectedNode) ?? null;
  const tone = latencyTone(selected?.latencyMs);

  const openCreate = () => {
    setEditing(null);
    setSheetOpen(true);
  };

  const openEdit = (node: Node) => {
    setEditing(node);
    setSheetOpen(true);
  };

  const runTest = async (node: Node) => {
    setTestingId(node.id);
    await testLatency(node.id);
    setTestingId(null);
  };

  return (
    <div className="animate-fade-up px-6 py-5">
      <PageHeader
        title="节点"
        description="VMess 节点集中管理，选中即用于全部代理流量"
        actions={
          <>
            <Button
              icon={<Zap size={13} />}
              loading={busy}
              onClick={() => void testAll()}
              disabled={nodes.length === 0}
            >
              全部测速
            </Button>
            <Button variant="primary" icon={<Plus size={13} />} onClick={openCreate}>
              新增节点
            </Button>
          </>
        }
      />

      <div className="card mb-3 flex items-center gap-4 px-4 py-3">
        <div className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-accent-soft text-accent">
          <Server size={17} />
        </div>
        <div className="min-w-0 flex-1">
          <p className="text-[11px] text-ink-faint">当前节点</p>
          <p className="mt-0.5 truncate text-[13px] text-ink">
            {selected ? selected.name : "未选择"}
            {selected && (
              <span className="ml-2 font-mono text-[11px] text-ink-faint">
                {selected.host}:{selected.port}
              </span>
            )}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-4 text-right">
          <div>
            <p className="text-[10px] text-ink-faint">延迟</p>
            <p className="mt-0.5 flex items-center justify-end gap-1.5 font-mono text-[12px] text-ink">
              <span className={`h-1.5 w-1.5 rounded-full ${DOT[tone]}`} />
              {formatLatency(selected?.latencyMs)}
            </p>
          </div>
          <div className="h-7 w-px bg-white/[0.07]" />
          <div>
            <p className="text-[10px] text-ink-faint">状态</p>
            <p className={`mt-0.5 text-[12px] ${status?.running ? "text-ok" : "text-ink-muted"}`}>
              {status?.running ? "使用中" : "待命"}
            </p>
          </div>
        </div>
      </div>

      {nodes.length === 0 ? (
        <EmptyState
          icon={Server}
          title="还没有节点"
          description="粘贴一条 vmess:// 分享链接即可自动填入全部参数，也可以在表单里手动填写服务器地址、UUID 与加密方式。"
          hint="vmess://eyJ2IjoiMiIsInBzIjoi..."
        />
      ) : (
        <div className="flex flex-col gap-2" role="radiogroup" aria-label="节点列表">
          {nodes.map((node) => (
            <NodeCard
              key={node.id}
              node={node}
              selected={node.id === config?.selectedNode}
              testing={testingId === node.id}
              onSelect={() => void selectNode(node.id)}
              onEdit={() => openEdit(node)}
              onDelete={() => setDeleting(node)}
              onTest={() => void runTest(node)}
            />
          ))}
        </div>
      )}

      <p className="mt-3 px-1 text-[11px] leading-5 text-ink-faint">
        共 {nodes.length} 个节点 · 延迟为 TCP 握手耗时，仅用于横向比较 · 传输方式
        ws / grpc 尚未支持，导入时会给出提示
      </p>

      <NodeSheet open={sheetOpen} node={editing} onClose={() => setSheetOpen(false)} />

      <ConfirmDialog
        open={Boolean(deleting)}
        title={`删除节点「${deleting?.name ?? ""}」`}
        description="删除后该节点的 UUID 与配置将不再保存。如果它正在使用中，代理会先自动断开。"
        confirmText="删除"
        loading={busy}
        onCancel={() => setDeleting(null)}
        onConfirm={() => {
          if (deleting) void deleteNode(deleting.id);
          setDeleting(null);
        }}
      />
    </div>
  );
}
