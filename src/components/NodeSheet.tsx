import { AlertTriangle, Link2, ListTree } from "lucide-react";
import { useEffect, useState } from "react";
import { cn } from "../lib/cn";
import { errorMessage, invoke } from "../lib/tauri";
import type { AddNodeResult, Node, NodeInput, Security, Transport } from "../lib/types";
import { useAppStore } from "../store/useAppStore";
import Button from "./Button";
import Sheet from "./Sheet";
import Switch from "./Switch";

const TRANSPORTS: Transport[] = ["tcp", "ws", "grpc", "h2", "quic", "kcp", "http"];
const SECURITIES: Security[] = ["aes-128-gcm", "chacha20-poly1305", "legacy"];

function blank(): NodeInput {
  return {
    id: null,
    name: "",
    host: "",
    port: 443,
    uuid: "",
    alterId: 0,
    security: "aes-128-gcm",
    transport: "tcp",
    udp: true,
  };
}

function toInput(node: Node): NodeInput {
  return {
    id: node.id,
    name: node.name,
    host: node.host,
    port: node.port,
    uuid: node.uuid,
    alterId: node.alterId,
    security: node.security,
    transport: node.transport,
    udp: node.udp,
  };
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <span className="field-label">{label}</span>
      {children}
      {hint && <span className="mt-1 block text-[10px] text-ink-faint">{hint}</span>}
    </label>
  );
}

export default function NodeSheet({
  open,
  node,
  onClose,
}: {
  open: boolean;
  node: Node | null;
  onClose: () => void;
}) {
  const busy = useAppStore((s) => s.busy);
  const previewMode = useAppStore((s) => s.preview);
  const saveNode = useAppStore((s) => s.saveNode);
  const addNodeFromLink = useAppStore((s) => s.addNodeFromLink);

  const [mode, setMode] = useState<"link" | "form">("link");
  const [link, setLink] = useState("");
  const [preview, setPreview] = useState<AddNodeResult | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [form, setForm] = useState<NodeInput>(blank());

  useEffect(() => {
    if (!open) return;
    setMode(node ? "form" : "link");
    setLink("");
    setPreview(null);
    setPreviewError(null);
    setForm(node ? toInput(node) : blank());
  }, [open, node]);

  // 粘贴后即时解析预览，400ms 防抖避免每次按键都请求后端。
  useEffect(() => {
    if (!open || mode !== "link") return;
    const value = link.trim();
    if (!value.startsWith("vmess://")) {
      setPreview(null);
      setPreviewError(null);
      return;
    }
    const timer = setTimeout(async () => {
      if (!previewMode) {
        setPreviewing(true);
        try {
          const result = await invoke<AddNodeResult>("preview_vmess_link", { link: value });
          setPreview(result);
          setPreviewError(null);
        } catch (err) {
          setPreview(null);
          setPreviewError(errorMessage(err));
        } finally {
          setPreviewing(false);
        }
        return;
      }
      setPreview(null);
      setPreviewError("浏览器预览模式无法解析 vmess://，请在桌面端粘贴");
    }, 400);
    return () => clearTimeout(timer);
  }, [link, mode, open, previewMode]);

  const update = <K extends keyof NodeInput>(key: K, value: NodeInput[K]) =>
    setForm((prev) => ({ ...prev, [key]: value }));

  const submit = async () => {
    if (mode === "link") {
      const result = await addNodeFromLink(link.trim());
      if (result) onClose();
      return;
    }
    const ok = await saveNode(form);
    if (ok) onClose();
  };

  const canSubmit =
    mode === "link" ? preview !== null || (previewMode && link.trim().startsWith("vmess://")) : Boolean(form.host && form.uuid);

  return (
    <Sheet
      open={open}
      onClose={onClose}
      title={node ? "编辑节点" : "新增节点"}
      description={
        node
          ? "修改后保存即生效，若该节点正在使用会重新建立连接"
          : "支持 vmess:// 链接导入，也可以手动填写全部参数"
      }
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            取消
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!canSubmit}
            onClick={() => void submit()}
          >
            {node ? "保存修改" : "保存节点"}
          </Button>
        </>
      }
    >
      {!node && (
        <div className="mb-4 flex gap-1 rounded-xl border border-hairline bg-black/20 p-1">
          <button
            type="button"
            onClick={() => setMode("link")}
            className={cn(
              "flex h-8 flex-1 items-center justify-center gap-1.5 rounded-lg text-[12px] transition-colors",
              mode === "link"
                ? "bg-white/[0.07] text-ink shadow-[inset_0_0_0_1px_rgba(255,255,255,0.06)]"
                : "text-ink-faint hover:text-ink-muted"
            )}
          >
            <Link2 size={13} /> 粘贴链接
          </button>
          <button
            type="button"
            onClick={() => setMode("form")}
            className={cn(
              "flex h-8 flex-1 items-center justify-center gap-1.5 rounded-lg text-[12px] transition-colors",
              mode === "form"
                ? "bg-white/[0.07] text-ink shadow-[inset_0_0_0_1px_rgba(255,255,255,0.06)]"
                : "text-ink-faint hover:text-ink-muted"
            )}
          >
            <ListTree size={13} /> 手动填写
          </button>
        </div>
      )}

      {mode === "link" ? (
        <div>
          <Field label="vmess:// 链接" hint="支持 v2rayN 分享链接与 Shadowrocket 旧格式">
            <textarea
              className="field-area h-[132px] font-mono text-[11px]"
              value={link}
              spellCheck={false}
              placeholder="vmess://eyJ2IjoiMiIsInBzIjoi6aaZ5rivIDAxIiwiYWRkIjoiaGsuZXhhbXBsZS5jb20i..." 
              onChange={(e) => setLink(e.target.value)}
            />
          </Field>

          {previewing && <p className="mt-3 text-[11px] text-ink-faint">正在解析…</p>}

          {previewError && (
            <div className="mt-3 flex items-start gap-2 rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-[11px] leading-5 text-danger">
              <AlertTriangle size={13} className="mt-0.5 shrink-0" /> {previewError}
            </div>
          )}

          {preview && (
            <div className="mt-3 rounded-xl border border-hairline bg-white/[0.03] p-3">
              <div className="flex items-center justify-between">
                <span className="text-[12px] text-ink">{preview.node.name}</span>
                <span className="rounded-md border border-hairline px-1.5 py-[1px] font-mono text-[10px] text-ink-faint">
                  {preview.node.transport}
                </span>
              </div>
              <p className="mt-1 font-mono text-[11px] text-ink-faint">
                {preview.node.host}:{preview.node.port}
              </p>
              <div className="mt-2 flex flex-wrap gap-1.5 text-[10px] text-ink-faint">
                <span className="rounded-md bg-white/[0.04] px-1.5 py-[1px]">
                  {preview.node.security}
                </span>
                <span className="rounded-md bg-white/[0.04] px-1.5 py-[1px]">
                  alterId {preview.node.alterId}
                </span>
                <span className="rounded-md bg-white/[0.04] px-1.5 py-[1px]">
                  UDP {preview.node.udp ? "开" : "关"}
                </span>
                <span className="rounded-md bg-white/[0.04] px-1.5 py-[1px]">
                  UUID {preview.node.uuid.slice(0, 8)}…
                </span>
              </div>
              {preview.warnings.map((warning) => (
                <div
                  key={warning}
                  className="mt-2 flex items-start gap-1.5 rounded-lg border border-warn/30 bg-warn/10 px-2 py-1.5 text-[10px] leading-4 text-warn"
                >
                  <AlertTriangle size={11} className="mt-0.5 shrink-0" />
                  {warning}
                </div>
              ))}
            </div>
          )}
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          <Field label="节点名称">
            <input
              className="field"
              value={form.name}
              placeholder="香港 · 01"
              onChange={(e) => update("name", e.target.value)}
            />
          </Field>

          <div className="flex gap-3">
            <div className="min-w-0 flex-1">
              <Field label="服务器地址">
                <input
                  className="field font-mono"
                  value={form.host}
                  spellCheck={false}
                  placeholder="hk1.example.com"
                  onChange={(e) => update("host", e.target.value)}
                />
              </Field>
            </div>
            <div className="w-[104px] shrink-0">
              <Field label="端口">
                <input
                  className="field font-mono"
                  type="number"
                  min={1}
                  max={65535}
                  value={form.port}
                  onChange={(e) => update("port", Number(e.target.value))}
                />
              </Field>
            </div>
          </div>

          <Field label="UUID" hint="标准 8-4-4-4-12 格式，也可不带连字符">
            <input
              className="field font-mono text-[11px]"
              value={form.uuid}
              spellCheck={false}
              placeholder="b831381d-6324-4d53-ad4f-8cda48b30811"
              onChange={(e) => update("uuid", e.target.value)}
            />
          </Field>

          <div className="flex gap-3">
            <div className="min-w-0 flex-1">
              <Field label="加密方式">
                <select
                  className="field"
                  value={form.security}
                  onChange={(e) => update("security", e.target.value as Security)}
                >
                  {SECURITIES.map((item) => (
                    <option key={item} value={item}>
                      {item}
                    </option>
                  ))}
                </select>
              </Field>
            </div>
            <div className="w-[112px] shrink-0">
              <Field label="alterId" hint="0 = AEAD，>0 = 旧版 VMess">
                <input
                  className="field font-mono"
                  type="number"
                  min={0}
                  max={65535}
                  value={form.alterId}
                  onChange={(e) => update("alterId", Number(e.target.value))}
                />
              </Field>
            </div>
          </div>

          <Field label="传输方式" hint="当前版本仅支持 tcp；ws / grpc 需要后续版本">
            <select
              className="field"
              value={form.transport}
              onChange={(e) => update("transport", e.target.value as Transport)}
            >
              {TRANSPORTS.map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </select>
          </Field>

          <div className="flex items-center justify-between gap-3 rounded-xl border border-hairline bg-white/[0.03] px-3 py-2.5">
            <div className="min-w-0">
              <span className="field-label">UDP 转发</span>
              <p className="mt-0.5 text-[10px] leading-4 text-ink-faint">
                仅对显式走 SOCKS5 的程序生效；系统代理本身不承载 UDP
              </p>
            </div>
            <Switch
              checked={form.udp}
              onChange={(next) => update("udp", next)}
              label="UDP 转发"
            />
          </div>

          {form.transport !== "tcp" && (
            <div className="flex items-start gap-2 rounded-lg border border-warn/30 bg-warn/10 px-3 py-2 text-[11px] leading-5 text-warn">
              <AlertTriangle size={13} className="mt-0.5 shrink-0" />
              当前版本只实现了 TCP 传输，保存为 {form.transport} 会被拒绝。
            </div>
          )}
        </div>
      )}
    </Sheet>
  );
}
