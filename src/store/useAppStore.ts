import { create } from "zustand";

import { demoBootstrap, demoHistory } from "../lib/mock";
import { errorMessage, invoke, isTauri } from "../lib/tauri";
import {
  checkForUpdate,
  downloadPendingUpdate,
  installDownloadedUpdate,
  type UpdateInfo,
} from "../lib/update";
import type {
  ActiveConnection,
  AppConfig,
  Bootstrap,
  HistoryPoint,
  LatencyResult,
  Node,
  NodeInput,
  ProxyStatus,
  RulesSummary,
  SettingsInput,
  TrafficPayload,
  TrafficStats,
} from "../lib/types";
import { useRuleStore } from "./useRuleStore";

export type ToastKind = "ok" | "error" | "info";

export interface Toast {
  id: number;
  message: string;
  kind: ToastKind;
}

const EMPTY_TRAFFIC: TrafficStats = {
  upTotal: 0,
  downTotal: 0,
  upRate: 0,
  downRate: 0,
  active: 0,
  totalConnections: 0,
};

export interface UpdateState {
  checking: boolean;
  available: UpdateInfo | null;
  installing: boolean;
  /** 下载进度 0-100。 */
  progress: number;
  error: string | null;
  /** 用户点了「稍后提醒」，本次会话不再弹提示。 */
  dismissed: boolean;
}

const EMPTY_UPDATE: UpdateState = {
  checking: false,
  available: null,
  installing: false,
  progress: 0,
  error: null,
  dismissed: false,
};

const HISTORY_LEN = 60;
let toastSeq = 0;
let previewTimer: ReturnType<typeof setInterval> | null = null;

interface AppStore {
  ready: boolean;
  preview: boolean;
  busy: boolean;
  config: AppConfig | null;
  status: ProxyStatus | null;
  rules: RulesSummary | null;
  traffic: TrafficStats;
  active: ActiveConnection[];
  history: HistoryPoint[];
  toasts: Toast[];
  update: UpdateState;

  init: () => Promise<void>;
  refresh: () => Promise<void>;
  toggle: () => Promise<void>;
  applyTraffic: (payload: TrafficPayload) => void;
  applyStatus: (status: ProxyStatus) => void;
  applyRules: (summary: RulesSummary) => void;

  addNodeFromLink: (link: string) => Promise<{ warnings: string[] } | null>;
  saveNode: (input: NodeInput) => Promise<boolean>;
  deleteNode: (id: string) => Promise<void>;
  selectNode: (id: string) => Promise<void>;
  testLatency: (id: string) => Promise<void>;
  testAll: () => Promise<void>;

  saveSettings: (input: SettingsInput) => Promise<boolean>;
  resetConfig: () => Promise<void>;
  clearRuleCache: () => Promise<void>;

  checkUpdate: (silent?: boolean) => Promise<void>;
  installUpdate: () => Promise<void>;
  dismissUpdate: () => void;

  toast: (message: string, kind?: ToastKind) => void;
  dismissToast: (id: number) => void;
}

function pushHistory(history: HistoryPoint[], stats: TrafficStats): HistoryPoint[] {
  const next = [...history, { up: stats.upRate, down: stats.downRate }];
  return next.length > HISTORY_LEN ? next.slice(next.length - HISTORY_LEN) : next;
}

function emptyHistory(): HistoryPoint[] {
  return Array.from({ length: HISTORY_LEN }, () => ({ up: 0, down: 0 }));
}

/** 设置保存后立即把受影响的运行态字段合并进去，避免等下一次事件才刷新首页。 */
function withSettingsStatus(
  status: ProxyStatus | null,
  input: SettingsInput
): ProxyStatus | null {
  if (!status) return status;
  return {
    ...status,
    localPort: input.listenPort,
    allowLan: input.allowLan,
    defaultPolicy: input.defaultPolicy,
    systemProxy: input.systemProxy,
    envProxy: input.proxyEnv,
  };
}

function withSettingsRules(
  rules: RulesSummary | null,
  input: SettingsInput
): RulesSummary | null {
  return rules ? { ...rules, defaultPolicy: input.defaultPolicy } : rules;
}

export const useAppStore = create<AppStore>((set, get) => ({
  ready: false,
  preview: false,
  busy: false,
  config: null,
  status: null,
  rules: null,
  traffic: EMPTY_TRAFFIC,
  active: [],
  history: emptyHistory(),
  toasts: [],
  update: EMPTY_UPDATE,

  init: async () => {
    let data: Bootstrap;
    try {
      data = isTauri ? await invoke<Bootstrap>("bootstrap") : demoBootstrap();
    } catch (err) {
      set({ ready: true, preview: !isTauri, toasts: [] });
      get().toast(errorMessage(err), "error");
      return;
    }

    set({
      ready: true,
      preview: !isTauri,
      config: data.config,
      status: data.status,
      rules: data.rules,
      traffic: data.stats,
      active: data.active,
      history: isTauri ? pushHistory(emptyHistory(), data.stats) : demoHistory(),
    });
    useRuleStore.getState().hydrate(data.config.customRules, data.config.remoteRules, data.rules);

    if (!isTauri) {
      startPreviewTicker(set, get);
    } else {
      // 启动后延迟检查，避免和首屏加载、规则编译抢带宽。
      setTimeout(() => void get().checkUpdate(true), 6000);
    }
  },

  refresh: async () => {
    if (!isTauri) return;
    try {
      const data = await invoke<Bootstrap>("bootstrap");
      set({
        config: data.config,
        status: data.status,
        rules: data.rules,
        traffic: data.stats,
        active: data.active,
        history: pushHistory(get().history, data.stats),
      });
      useRuleStore
        .getState()
        .hydrate(data.config.customRules, data.config.remoteRules, data.rules);
    } catch (err) {
      get().toast(errorMessage(err), "error");
    }
  },

  toggle: async () => {
    const { status, preview } = get();
    if (!status) return;
    set({ busy: true });
    try {
      if (preview) {
        const running = !status.running;
        set({
          status: {
            ...status,
            running,
            startedAt: running ? Math.floor(Date.now() / 1000) : null,
            systemProxy: running && (get().config?.systemProxy ?? true),
            envProxy: running && (get().config?.proxyEnv ?? false),
          },
          active: running ? demoBootstrap().active : [],
        });
        get().toast(running ? "已开启代理（预览模式）" : "已关闭代理", "ok");
        return;
      }
      const next = await invoke<ProxyStatus>(
        status.running ? "stop_proxy" : "start_proxy"
      );
      set({ status: next });
      get().toast(next.running ? "代理已开启，系统代理已接管" : "代理已关闭，系统代理已还原", "ok");
    } catch (err) {
      get().toast(errorMessage(err), "error");
    } finally {
      set({ busy: false });
    }
  },

  applyTraffic: (payload) => {
    const status = get().status;
    set({
      traffic: payload.stats,
      active: payload.active,
      history: pushHistory(get().history, payload.stats),
      status: status ? { ...status, running: payload.running } : status,
    });
  },

  applyStatus: (status) => {
    set({ status });
  },

  applyRules: (summary) => {
    set({ rules: summary });
  },

  addNodeFromLink: async (link) => {
    if (get().preview) {
      get().toast("预览模式下无法导入节点，请在桌面端使用", "info");
      return null;
    }
    set({ busy: true });
    try {
      const result = await invoke<{ node: Node; warnings: string[] }>(
        "add_node_from_link",
        { link }
      );
      const config = get().config;
      if (config) {
        const nodes = [...config.nodes, result.node];
        set({
          config: {
            ...config,
            nodes,
            selectedNode: config.selectedNode ?? result.node.id,
          },
        });
      }
      for (const warning of result.warnings) get().toast(warning, "info");
      get().toast(`已导入节点「${result.node.name}」`, "ok");
      return { warnings: result.warnings };
    } catch (err) {
      get().toast(errorMessage(err), "error");
      return null;
    } finally {
      set({ busy: false });
    }
  },

  saveNode: async (input) => {
    if (get().preview) {
      get().toast("预览模式下无法保存节点", "info");
      return false;
    }
    set({ busy: true });
    try {
      const node = await invoke<Node>("save_node", { input });
      const config = get().config;
      if (config) {
        const exists = config.nodes.some((n) => n.id === node.id);
        const nodes = exists
          ? config.nodes.map((n) => (n.id === node.id ? node : n))
          : [...config.nodes, node];
        set({
          config: {
            ...config,
            nodes,
            selectedNode: config.selectedNode ?? node.id,
          },
        });
      }
      get().toast("节点已保存", "ok");
      return true;
    } catch (err) {
      get().toast(errorMessage(err), "error");
      return false;
    } finally {
      set({ busy: false });
    }
  },

  deleteNode: async (id) => {
    if (get().preview) {
      const config = get().config;
      if (!config) return;
      const nodes = config.nodes.filter((n) => n.id !== id);
      set({
        config: {
          ...config,
          nodes,
          selectedNode:
            config.selectedNode === id ? (nodes[0]?.id ?? null) : config.selectedNode,
        },
      });
      return;
    }
    set({ busy: true });
    try {
      const nodes = await invoke<Node[]>("delete_node", { id });
      const config = get().config;
      if (config) {
        set({
          config: {
            ...config,
            nodes,
            selectedNode:
              config.selectedNode === id ? (nodes[0]?.id ?? null) : config.selectedNode,
          },
        });
      }
      get().toast("节点已删除", "ok");
    } catch (err) {
      get().toast(errorMessage(err), "error");
    } finally {
      set({ busy: false });
    }
  },

  selectNode: async (id) => {
    const config = get().config;
    if (!config || config.selectedNode === id) return;
    if (get().preview) {
      set({ config: { ...config, selectedNode: id } });
      return;
    }
    set({ busy: true });
    try {
      await invoke<void>("select_node", { id });
      set({ config: { ...get().config!, selectedNode: id } });
      const status = await invoke<ProxyStatus>("get_status");
      set({ status });
      get().toast("已切换节点", "ok");
    } catch (err) {
      get().toast(errorMessage(err), "error");
    } finally {
      set({ busy: false });
    }
  },

  testLatency: async (id) => {
    if (get().preview) {
      const config = get().config;
      if (!config) return;
      const latency = 40 + Math.floor(Math.random() * 220);
      set({
        config: {
          ...config,
          nodes: config.nodes.map((n) => (n.id === id ? { ...n, latencyMs: latency } : n)),
        },
      });
      return;
    }
    try {
      const result = await invoke<LatencyResult>("test_node_latency", { id });
      const config = get().config;
      if (config) {
        set({
          config: {
            ...config,
            nodes: config.nodes.map((n) =>
              n.id === id ? { ...n, latencyMs: result.latencyMs } : n
            ),
          },
        });
      }
      if (result.error) get().toast(`${result.name}：${result.error}`, "error");
    } catch (err) {
      get().toast(errorMessage(err), "error");
    }
  },

  testAll: async () => {
    if (get().preview) {
      const config = get().config;
      if (!config) return;
      set({
        config: {
          ...config,
          nodes: config.nodes.map((n) => ({
            ...n,
            latencyMs: 40 + Math.floor(Math.random() * 240),
          })),
        },
      });
      get().toast("已完成全部节点测速（预览模式）", "ok");
      return;
    }
    set({ busy: true });
    try {
      const results = await invoke<LatencyResult[]>("test_all_latencies");
      const config = get().config;
      if (config) {
        set({
          config: {
            ...config,
            nodes: config.nodes.map((n) => {
              const hit = results.find((r) => r.id === n.id);
              return hit ? { ...n, latencyMs: hit.latencyMs } : n;
            }),
          },
        });
      }
      const failed = results.filter((r) => r.error).length;
      get().toast(
        failed > 0 ? `测速完成，${failed} 个节点不可用` : "全部节点测速完成",
        failed > 0 ? "info" : "ok"
      );
    } catch (err) {
      get().toast(errorMessage(err), "error");
    } finally {
      set({ busy: false });
    }
  },

  saveSettings: async (input) => {
    const before = {
      config: get().config,
      status: get().status,
      rules: get().rules,
    };
    const config = before.config;
    if (!config) return false;

    const nextStatus = withSettingsStatus(before.status, input);
    const nextRules = withSettingsRules(before.rules, input);

    if (get().preview) {
      set({ config: { ...config, ...input }, status: nextStatus, rules: nextRules });
      if (nextRules) useRuleStore.getState().applySummary(nextRules);
      get().toast("设置已保存（预览模式）", "ok");
      return true;
    }

    // 先更新 UI，再等待后端；端口 / 系统代理等需要重启的设置在返回后会由事件校正。
    set({
      busy: true,
      config: { ...config, ...input },
      status: nextStatus,
      rules: nextRules,
    });
    try {
      const status = await invoke<ProxyStatus>("update_settings", { input });
      set({
        config: { ...get().config!, ...input },
        status,
        rules: withSettingsRules(get().rules, input),
      });
      if (nextRules) useRuleStore.getState().applySummary(get().rules!);
      get().toast("设置已保存", "ok");
      return true;
    } catch (err) {
      const message = errorMessage(err);
      // update_settings 可能已经落盘（例如端口被占用时配置已写入），
      // 重新拉取后端真实状态，避免 UI 回滚到与磁盘不一致的旧配置。
      set(before);
      await get().refresh();
      get().toast(message, "error");
      return false;
    } finally {
      set({ busy: false });
    }
  },

  resetConfig: async () => {
    if (get().preview) {
      const data = demoBootstrap();
      set({ config: { ...data.config, nodes: [], remoteRules: [], selectedNode: null } });
      get().toast("已恢复默认配置（预览模式）", "ok");
      return;
    }
    set({ busy: true });
    try {
      await invoke<ProxyStatus>("reset_config");
      await get().refresh();
      get().toast("已恢复默认配置", "ok");
    } catch (err) {
      get().toast(errorMessage(err), "error");
    } finally {
      set({ busy: false });
    }
  },

  clearRuleCache: async () => {
    if (get().preview) {
      get().toast("已清空规则缓存（预览模式）", "ok");
      return;
    }
    set({ busy: true });
    try {
      const summary = await invoke<RulesSummary>("clear_rule_cache");
      set({ rules: summary });
      useRuleStore.getState().applySummary(summary);
      await get().refresh();
      get().toast("规则缓存已清空", "ok");
    } catch (err) {
      get().toast(errorMessage(err), "error");
    } finally {
      set({ busy: false });
    }
  },

  checkUpdate: async (silent = false) => {
    if (get().preview) {
      if (!silent) get().toast("浏览器预览模式无法检查更新", "info");
      return;
    }
    if (get().update.checking || get().update.installing) return;

    set({ update: { ...get().update, checking: true, error: null } });
    try {
      const info = await checkForUpdate();
      set({ update: { ...get().update, checking: false, available: info } });
      if (!info) {
        if (!silent) get().toast("已是最新版本", "ok");
      } else if (!get().update.dismissed) {
        get().toast(`发现新版本 v${info.version}，可在设置页更新`, "info");
      }
    } catch (err) {
      const message = errorMessage(err);
      set({ update: { ...get().update, checking: false, error: message } });
      if (!silent) get().toast(message, "error");
    }
  },

  installUpdate: async () => {
    const { update } = get();
    if (!update.available || update.installing) return;

    // 先上锁，避免下载 / 停代理期间重复点击触发两次安装。
    set({ update: { ...update, installing: true, progress: 0, error: null } });
    try {
      await downloadPendingUpdate((progress) =>
        set({ update: { ...get().update, progress: progress.percent } })
      );

      // 安装器要替换正在运行的 exe，下载完成后再停掉入站并还原系统代理。
      if (get().status?.running) {
        const next = await invoke<ProxyStatus>("stop_proxy");
        set({ status: next });
      }

      await installDownloadedUpdate();
    } catch (err) {
      const message = errorMessage(err);
      set({ update: { ...get().update, installing: false, error: message } });
      get().toast(message, "error");
    }
  },

  dismissUpdate: () => set({ update: { ...get().update, dismissed: true } }),

  toast: (message, kind = "info") => {
    const id = ++toastSeq;
    set({ toasts: [...get().toasts, { id, message, kind }] });
    setTimeout(() => get().dismissToast(id), 3800);
  },

  dismissToast: (id) => {
    set({ toasts: get().toasts.filter((t) => t.id !== id) });
  },
}));

/** 浏览器预览：造一点波动，方便核对曲线与动画。 */
function startPreviewTicker(
  set: (partial: Partial<AppStore>) => void,
  get: () => AppStore
) {
  if (previewTimer) return;
  previewTimer = setInterval(() => {
    const { status, traffic } = get();
    if (!status?.running) {
      set({ traffic: { ...traffic, upRate: 0, downRate: 0, active: 0 } });
      return;
    }
    const upRate = Math.round(8_000 + Math.random() * 26_000);
    const downRate = Math.round(120_000 + Math.random() * 620_000);
    const next: TrafficStats = {
      upTotal: traffic.upTotal + upRate,
      downTotal: traffic.downTotal + downRate,
      upRate,
      downRate,
      active: demoBootstrap().active.length,
      totalConnections: traffic.totalConnections + (Math.random() > 0.7 ? 1 : 0),
    };
    set({
      traffic: next,
      history: pushHistory(get().history, next),
      active: demoBootstrap().active,
    });
  }, 1000);
}
