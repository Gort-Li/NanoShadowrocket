import {
  Database,
  Download,
  Globe2,
  Info,
  Network,
  RefreshCw,
  RotateCcw,
  Save,
  Shield,
  Wifi,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import Button from "../components/Button";
import ConfirmDialog from "../components/ConfirmDialog";
import PageHeader from "../components/PageHeader";
import SettingRow from "../components/SettingRow";
import Switch from "../components/Switch";
import { POLICY_LABEL } from "../lib/format";
import { appVersion } from "../lib/tauri";
import type { AppConfig, Policy, SettingsInput } from "../lib/types";
import { useAppStore } from "../store/useAppStore";

const DNS_PRESETS = [
  { value: "system", label: "跟随系统 DNS" },
  { value: "https://cloudflare-dns.com/dns-query", label: "Cloudflare DoH（1.1.1.1）" },
  { value: "https://dns.google/resolve", label: "Google DoH（8.8.8.8）" },
];

function fromConfig(config: AppConfig): SettingsInput {
  return {
    listenPort: config.listenPort,
    allowLan: config.allowLan,
    defaultPolicy: config.defaultPolicy,
    systemProxy: config.systemProxy,
    proxyEnv: config.proxyEnv,
    launchAtStartup: config.launchAtStartup,
    autoStartProxy: config.autoStartProxy,
    dnsServer: config.dnsServer,
    ipv6: config.ipv6,
  };
}

function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: Array<{ value: T; label: string }>;
  onChange: (next: T) => void;
}) {
  return (
    <div className="flex gap-1 rounded-[10px] border border-hairline bg-black/20 p-0.5">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          onClick={() => onChange(option.value)}
          className={
            value === option.value
              ? "rounded-lg bg-white/[0.09] px-2.5 py-1 text-[11px] text-ink shadow-[inset_0_0_0_1px_rgba(255,255,255,0.07)]"
              : "rounded-lg px-2.5 py-1 text-[11px] text-ink-faint transition-colors hover:text-ink-muted"
          }
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function Group({
  icon,
  title,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="card overflow-hidden">
      <header className="flex items-center gap-2 border-b border-hairline px-4 py-2.5 text-ink-muted">
        {icon}
        <h2 className="text-[12px] font-medium">{title}</h2>
      </header>
      <div className="divide-y divide-white/[0.05]">{children}</div>
    </section>
  );
}

export default function SettingsPage() {
  const config = useAppStore((s) => s.config);
  const status = useAppStore((s) => s.status);
  const busy = useAppStore((s) => s.busy);
  const saveSettings = useAppStore((s) => s.saveSettings);
  const clearRuleCache = useAppStore((s) => s.clearRuleCache);
  const resetConfig = useAppStore((s) => s.resetConfig);
  const updateState = useAppStore((s) => s.update);
  const checkUpdate = useAppStore((s) => s.checkUpdate);
  const installUpdate = useAppStore((s) => s.installUpdate);

  const [draft, setDraft] = useState<SettingsInput | null>(null);
  const [confirmCache, setConfirmCache] = useState(false);
  const [confirmReset, setConfirmReset] = useState(false);
  const [customDns, setCustomDns] = useState(false);
  const [version, setVersion] = useState("");

  useEffect(() => {
    void appVersion().then(setVersion);
  }, []);

  useEffect(() => {
    if (!config) return;
    setDraft(fromConfig(config));
    setCustomDns(!DNS_PRESETS.some((preset) => preset.value === config.dnsServer));
  }, [config]);

  const dirty = useMemo(() => {
    if (!config || !draft) return false;
    return JSON.stringify(fromConfig(config)) !== JSON.stringify(draft);
  }, [config, draft]);

  if (!draft) {
    return (
      <div className="px-6 py-5">
        <PageHeader title="设置" description="正在读取配置…" />
        <div className="card px-4 py-10 text-center text-[12px] text-ink-faint">
          配置尚未加载完成
        </div>
      </div>
    );
  }

  const update = (patch: Partial<SettingsInput>) =>
    setDraft((prev) => (prev ? { ...prev, ...patch } : prev));

  /** 开关类改动立即落盘并生效（后端会自动重启入站）。 */
  const applyNow = (patch: Partial<SettingsInput>) => {
    const next = { ...draft, ...patch };
    setDraft(next);
    void saveSettings(next);
  };

  return (
    <div className="animate-fade-up px-6 py-5">
      <PageHeader
        title="设置"
        description="代理、网络与系统集成选项，修改后立即生效"
        actions={
          <Button
            variant={dirty ? "primary" : "secondary"}
            icon={<Save size={13} />}
            loading={busy}
            disabled={!dirty}
            onClick={() => void saveSettings(draft)}
          >
            {dirty ? "保存修改" : "已保存"}
          </Button>
        }
      />

      <div className="flex flex-col gap-3">
        <Group icon={<Shield size={13} />} title="代理">
          <SettingRow label="本地端口" description="HTTP 与 SOCKS5 共用此端口，修改后会自动重启监听">
            <input
              type="number"
              min={1}
              max={65535}
              value={draft.listenPort}
              aria-label="本地端口"
              onChange={(e) => update({ listenPort: Number(e.target.value) })}
              className="field w-[92px] text-right font-mono"
            />
          </SettingRow>

          <SettingRow label="默认策略" description="没有命中任何规则（含 FINAL）时的兜底走向">
            <Segmented<Policy>
              value={draft.defaultPolicy}
              options={[
                { value: "proxy", label: POLICY_LABEL.proxy },
                { value: "direct", label: POLICY_LABEL.direct },
                { value: "reject", label: POLICY_LABEL.reject },
              ]}
              onChange={(next) => applyNow({ defaultPolicy: next })}
            />
          </SettingRow>

          <SettingRow
            label="允许局域网连接"
            description={`当前监听 ${draft.allowLan ? "0.0.0.0" : "127.0.0.1"}:${draft.listenPort}`}
          >
            <Switch
              checked={draft.allowLan}
              label="允许局域网连接"
              onChange={(next) => applyNow({ allowLan: next })}
            />
          </SettingRow>
        </Group>

        <Group icon={<Globe2 size={13} />} title="网络">
          <SettingRow label="DNS 服务器" description="仅在 IP-CIDR / GEOIP 规则需要解析时使用">
            <select
              className="field w-[220px] text-[11px]"
              value={customDns ? "custom" : draft.dnsServer}
              onChange={(e) => {
                if (e.target.value === "custom") {
                  setCustomDns(true);
                  return;
                }
                setCustomDns(false);
                update({ dnsServer: e.target.value });
              }}
            >
              {DNS_PRESETS.map((preset) => (
                <option key={preset.value} value={preset.value}>
                  {preset.label}
                </option>
              ))}
              <option value="custom">自定义 DoH 地址</option>
            </select>
          </SettingRow>
          {customDns && (
            <SettingRow label="DoH 地址" description="必须是 https:// 开头的 JSON API 端点">
              <input
                className="field w-[220px] font-mono text-[11px]"
                value={draft.dnsServer}
                spellCheck={false}
                onChange={(e) => update({ dnsServer: e.target.value })}
              />
            </SettingRow>
          )}
          <SettingRow label="IPv6 解析" description="关闭后只查询 A 记录，避免无 IPv6 出口时超时">
            <Switch
              checked={draft.ipv6}
              label="IPv6 解析"
              onChange={(next) => applyNow({ ipv6: next })}
            />
          </SettingRow>
        </Group>

        <Group icon={<Wifi size={13} />} title="系统">
          <SettingRow
            label="开机自动启动"
            description="写入当前用户启动项，不修改系统级设置"
          >
            <Switch
              checked={draft.launchAtStartup}
              label="开机自动启动"
              onChange={(next) => applyNow({ launchAtStartup: next })}
            />
          </SettingRow>
          <SettingRow
            label="开机后自动连接"
            description={
              draft.launchAtStartup
                ? "开机自启时静默驻留托盘，并自动开启上次使用的节点"
                : "需先开启「开机自动启动」，该项才会生效"
            }
          >
            <Switch
              checked={draft.autoStartProxy}
              label="开机后自动连接"
              disabled={!draft.launchAtStartup}
              onChange={(next) => applyNow({ autoStartProxy: next })}
            />
          </SettingRow>
          <SettingRow
            label="接管系统代理"
            description={
              status?.running
                ? status.systemProxy
                  ? "当前已接管，退出或关闭代理时还原"
                  : "本次会话未接管，仅在本地端口监听"
                : "开启代理时写入 Windows 代理设置"
            }
          >
            <Switch
              checked={draft.systemProxy}
              label="接管系统代理"
              onChange={(next) => applyNow({ systemProxy: next })}
            />
          </SettingRow>
          <SettingRow
            label="命令行代理环境变量"
            description={
              status?.running
                ? status.envProxy
                  ? "已写入 HTTP_PROXY / HTTPS_PROXY，新开的终端生效"
                  : "本次会话未写入，命令行工具不会走代理"
                : "写入用户变量，让 curl、git、npm 等命令行工具也走代理"
            }
          >
            <Switch
              checked={draft.proxyEnv}
              label="命令行代理环境变量"
              onChange={(next) => applyNow({ proxyEnv: next })}
            />
          </SettingRow>
        </Group>

        <Group icon={<Database size={13} />} title="数据与危险操作">
          <SettingRow
            label="清除规则缓存"
            description="删除已下载的远程规则正文，下次更新重新拉取"
            onClick={() => setConfirmCache(true)}
          >
            <span className="cursor-pointer text-[11px] text-ink-faint">清除</span>
          </SettingRow>
          <SettingRow
            label="重置全部配置"
            description="删除节点、订阅与自定义规则，并还原默认设置"
            danger
            onClick={() => setConfirmReset(true)}
          >
            <span className="flex cursor-pointer items-center gap-1 text-[11px] text-danger">
              <RotateCcw size={11} /> 重置
            </span>
          </SettingRow>
        </Group>

        <Group icon={<Info size={13} />} title="关于">
          <SettingRow
            label="当前版本"
            description="安装包由 minisign 签名校验，更新源为 GitHub Releases"
          >
            <span className="font-mono text-[11px] text-ink-muted">
              {version ? `v${version}` : "…"}
            </span>
          </SettingRow>
          <SettingRow
            label="软件更新"
            description={
              updateState.installing
                ? updateState.progress >= 100
                  ? "安装包已下载，正在启动安装程序…"
                  : `正在下载安装包… ${updateState.progress}%`
                : updateState.available
                  ? `可更新到 v${updateState.available.version}，安装完成后会自动重启`
                  : updateState.checking
                    ? "正在检查…"
                    : (updateState.error ?? "检查更新只读取 latest.json，需要联网")
            }
          >
            {updateState.available ? (
              <Button
                variant="primary"
                icon={<Download size={13} />}
                loading={updateState.installing}
                onClick={() => void installUpdate()}
              >
                立即更新
              </Button>
            ) : (
              <Button
                icon={<RefreshCw size={13} />}
                loading={updateState.checking}
                onClick={() => void checkUpdate()}
              >
                检查
              </Button>
            )}
          </SettingRow>
          {updateState.installing && (
            <div className="px-4 py-3">
              <div className="h-1 overflow-hidden rounded-full bg-white/[0.06]">
                <div
                  className="h-full rounded-full bg-accent transition-[width] duration-300"
                  style={{ width: `${Math.max(4, updateState.progress)}%` }}
                />
              </div>
            </div>
          )}
        </Group>

        <div className="flex items-start gap-2 rounded-xl border border-hairline bg-white/[0.02] px-3.5 py-3 text-[11px] leading-5 text-ink-faint">
          <Network size={13} className="mt-0.5 shrink-0" />
          <p>
            系统代理只在开启代理时写入 <span className="font-mono">Internet Settings</span>
            ，关闭、退出、异常恢复与卸载这四条路径都会还原为启动前的快照，不影响其它系统设置。
            命令行环境变量同理，卸载前会被清理。
          </p>
        </div>
      </div>

      <ConfirmDialog
        open={confirmCache}
        title="清除规则缓存"
        description="已下载的远程规则文件会被删除，订阅本身与更新间隔保留。清除后规则集将只剩自定义规则与内置直连规则，直到下次更新。"
        confirmText="清除缓存"
        danger={false}
        loading={busy}
        onCancel={() => setConfirmCache(false)}
        onConfirm={() => {
          setConfirmCache(false);
          void clearRuleCache();
        }}
      />

      <ConfirmDialog
        open={confirmReset}
        title="重置全部配置"
        description="这会停止代理、删除所有节点与规则订阅，并恢复出厂设置，操作不可撤销。"
        confirmText="全部重置"
        loading={busy}
        onCancel={() => setConfirmReset(false)}
        onConfirm={() => {
          setConfirmReset(false);
          void resetConfig();
        }}
      />
    </div>
  );
}
