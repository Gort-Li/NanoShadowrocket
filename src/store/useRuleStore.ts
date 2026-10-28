import { create } from "zustand";

import {
  parseCustomRules,
  serializeCustomRules,
  upsertCustomRule,
  type CustomRule,
} from "../lib/customRules";
import { POLICY_LABEL } from "../lib/format";
import { demoRuleTest } from "../lib/mock";
import { errorMessage, invoke, isTauri } from "../lib/tauri";
import type { RemoteOutcome, RemoteRule, RuleTestResult, RulesSummary } from "../lib/types";
import { useAppStore } from "./useAppStore";

interface RuleStore {
  custom: string;
  remotes: RemoteRule[];
  summary: RulesSummary | null;
  testInput: string;
  testResult: RuleTestResult | null;
  testing: boolean;
  saving: boolean;

  hydrate: (custom: string, remotes: RemoteRule[], summary: RulesSummary) => void;
  setCustom: (text: string) => void;
  resetCustom: () => void;
  saveCustom: () => Promise<void>;
  saveCustomText: (text: string, success: string) => Promise<void>;
  applyQuickRule: (rule: CustomRule) => Promise<void>;

  addRemote: (name: string, url: string, intervalHours: number) => Promise<boolean>;
  removeRemote: (id: string) => Promise<void>;
  updateRemote: (id: string) => Promise<void>;
  updateAll: () => Promise<void>;
  setRemoteInterval: (id: string, hours: number) => Promise<void>;
  setRemoteEnabled: (id: string, enabled: boolean) => Promise<void>;
  reorder: (ids: string[]) => Promise<void>;

  setTestInput: (value: string) => void;
  runTest: (input?: string) => Promise<void>;
  applySummary: (summary: RulesSummary) => void;
  applyRemote: (outcome: RemoteOutcome) => void;
  previewMode: () => boolean;
}

function toast(message: string, kind: "ok" | "error" | "info" = "info") {
  useAppStore.getState().toast(message, kind);
}

/**
 * 自定义规则保存串行化。
 *
 * 以前并发保存会被 `if (saving) return` 直接丢掉：本地文本已经改了、后端还是旧的，
 * 表现为「规则点了没生效」。现在按点击顺序排队，最后一次写入一定是最终文本。
 */
let saveChain: Promise<void> = Promise.resolve();
let pendingSaves = 0;

export const useRuleStore = create<RuleStore>((set, get) => ({
  custom: "",
  remotes: [],
  summary: null,
  testInput: "",
  testResult: null,
  testing: false,
  saving: false,

  hydrate: (custom, remotes, summary) => {
    set({ custom, remotes, summary });
  },

  setCustom: (text) => set({ custom: text }),

  resetCustom: () => {
    set({
      custom:
        "# 自定义规则优先级最高，从上到下匹配，命中即止。\n# DOMAIN-SUFFIX,example.com,Proxy\n",
    });
  },

  saveCustom: async () => {
    await get().saveCustomText(get().custom, "自定义规则已生效");
  },

  saveCustomText: async (text, success) => {
    set({ custom: text });
    if (get().previewMode()) {
      const current = get().summary;
      if (current) {
        const count = parseCustomRules(text).rules.length;
        set({
          summary: {
            ...current,
            custom: count,
            total: Math.max(0, current.total + count - current.custom),
          },
        });
      }
      toast(`${success}（预览模式）`, "ok");
      return;
    }
    pendingSaves += 1;
    set({ saving: true });
    const task = saveChain.then(async () => {
      try {
        const summary = await invoke<RulesSummary>("save_custom_rules", { text });
        set({ summary });
        useAppStore.getState().applyRules(summary);
        toast(success, "ok");
      } catch (err) {
        toast(errorMessage(err), "error");
      } finally {
        pendingSaves -= 1;
        if (pendingSaves === 0) set({ saving: false });
      }
    });
    saveChain = task;
    await task;
  },

  applyQuickRule: async (rule) => {
    const parsed = parseCustomRules(get().custom);
    const next = upsertCustomRule(parsed.rules, rule);
    const text = serializeCustomRules(next, parsed.unsupported);
    const target = rule.kind === "FINAL" ? "FINAL" : `${rule.kind},${rule.value}`;
    await get().saveCustomText(
      text,
      `已写入 ${target} → ${POLICY_LABEL[rule.policy]}，新连接立即生效`
    );
  },

  addRemote: async (name, url, intervalHours) => {
    if (!url.trim().startsWith("http")) {
      toast("规则地址必须以 http:// 或 https:// 开头", "error");
      return false;
    }
    if (!isTauri) {
      const remote: RemoteRule = {
        id: `demo-${Date.now()}`,
        name: name.trim() || url.replace(/^https?:\/\//, "").split("/")[0],
        url: url.trim(),
        intervalHours,
        enabled: true,
        etag: null,
        lastModified: null,
        updatedAt: Math.floor(Date.now() / 1000),
        ruleCount: 0,
        ignoredCount: 0,
        status: "idle",
        error: null,
      };
      set({ remotes: [...get().remotes, remote] });
      toast("订阅已添加（预览模式）", "ok");
      return true;
    }
    try {
      const remote = await invoke<RemoteRule>("add_remote_rule", {
        input: { name, url, intervalHours },
      });
      set({ remotes: [...get().remotes, remote] });
      toast(
        remote.error ? `订阅已添加，但拉取失败：${remote.error}` : `已导入 ${remote.ruleCount} 条规则`,
        remote.error ? "error" : "ok"
      );
      return true;
    } catch (err) {
      toast(errorMessage(err), "error");
      return false;
    }
  },

  removeRemote: async (id) => {
    if (!isTauri) {
      set({ remotes: get().remotes.filter((r) => r.id !== id) });
      return;
    }
    try {
      const summary = await invoke<RulesSummary>("remove_remote_rule", { id });
      set({ remotes: get().remotes.filter((r) => r.id !== id), summary });
      useAppStore.getState().applyRules(summary);
      toast("订阅已删除", "ok");
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  },

  updateRemote: async (id) => {
    if (!isTauri) {
      set({
        remotes: get().remotes.map((r) =>
          r.id === id
            ? {
                ...r,
                status: "ok",
                updatedAt: Math.floor(Date.now() / 1000),
                ruleCount: r.ruleCount || 1024,
                error: null,
              }
            : r
        ),
      });
      toast("规则已更新（预览模式）", "ok");
      return;
    }
    try {
      const outcome = await invoke<RemoteOutcome>("update_remote_rule", { id });
      get().applyRemote(outcome);
      if (outcome.error) toast(outcome.error, "error");
      else toast(outcome.changed ? `已更新 ${outcome.ruleCount} 条规则` : "已是最新规则", "ok");
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  },

  updateAll: async () => {
    if (!isTauri) {
      await Promise.all(get().remotes.map((r) => get().updateRemote(r.id)));
      return;
    }
    try {
      const outcomes = await invoke<RemoteOutcome[]>("update_all_remote_rules");
      for (const outcome of outcomes) get().applyRemote(outcome);
      const list = await invoke<RemoteRule[]>("list_remote_rules");
      set({ remotes: list });
      const failed = outcomes.filter((o) => o.error).length;
      toast(failed > 0 ? `${failed} 个订阅更新失败` : "全部订阅已更新", failed > 0 ? "error" : "ok");
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  },

  setRemoteInterval: async (id, hours) => {
    set({
      remotes: get().remotes.map((r) => (r.id === id ? { ...r, intervalHours: hours } : r)),
    });
    if (!isTauri) return;
    try {
      await invoke<RemoteRule[]>("set_remote_interval", { id, hours });
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  },

  setRemoteEnabled: async (id, enabled) => {
    set({
      remotes: get().remotes.map((r) => (r.id === id ? { ...r, enabled } : r)),
    });
    if (!isTauri) return;
    try {
      const summary = await invoke<RulesSummary>("set_remote_enabled", { id, enabled });
      set({ summary });
      useAppStore.getState().applyRules(summary);
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  },

  reorder: async (ids) => {
    const map = new Map(get().remotes.map((r) => [r.id, r]));
    const ordered = ids
      .map((id) => map.get(id))
      .filter((r): r is RemoteRule => Boolean(r))
      .concat(get().remotes.filter((r) => !ids.includes(r.id)));
    set({ remotes: ordered });
    if (!isTauri) return;
    try {
      const summary = await invoke<RulesSummary>("reorder_remote_rules", { ids });
      set({ summary });
      useAppStore.getState().applyRules(summary);
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  },

  setTestInput: (value) => set({ testInput: value }),

  runTest: async (input) => {
    const value = (input ?? get().testInput).trim();
    if (!value) {
      set({ testResult: null });
      return;
    }
    set({ testing: true });
    try {
      const result = isTauri
        ? await invoke<RuleTestResult>("test_rule", { input: value })
        : demoRuleTest(value);
      set({ testResult: result });
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      set({ testing: false });
    }
  },

  applySummary: (summary) => {
    set({ summary });
  },

  applyRemote: (outcome) => {
    set({
      remotes: get().remotes.map((r) =>
        r.id === outcome.id
          ? {
              ...r,
              status: outcome.error ? "error" : "ok",
              error: outcome.error,
              ruleCount: outcome.ruleCount,
              ignoredCount: outcome.ignoredCount,
              updatedAt: outcome.changed ? Math.floor(Date.now() / 1000) : r.updatedAt,
            }
          : r
      ),
    });
  },

  previewMode: () => !isTauri,
}));
