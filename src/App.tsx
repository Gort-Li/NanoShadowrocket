import { useEffect, useState } from "react";
import NavRail, { type PageKey } from "./components/NavRail";
import TitleBar from "./components/TitleBar";
import Toaster from "./components/Toast";
import HomePage from "./pages/HomePage";
import NodesPage from "./pages/NodesPage";
import RulesPage from "./pages/RulesPage";
import SettingsPage from "./pages/SettingsPage";
import { subscribe } from "./lib/tauri";
import type {
  ProxyStatus,
  RemoteOutcome,
  RulesSummary,
  TrafficPayload,
} from "./lib/types";
import { useAppStore } from "./store/useAppStore";
import { useRuleStore } from "./store/useRuleStore";

function LoadingScreen() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3">
      <div className="h-9 w-9 animate-spin rounded-full border-2 border-white/10 border-t-accent" />
      <p className="text-[12px] text-ink-faint">正在加载配置与规则…</p>
    </div>
  );
}

const PAGES: PageKey[] = ["home", "nodes", "rules", "settings"];

function pageFromHash(): PageKey {
  const raw = window.location.hash.replace(/^#\/?/, "");
  return PAGES.includes(raw as PageKey) ? (raw as PageKey) : "home";
}

export default function App() {
  const [page, setPage] = useState<PageKey>(pageFromHash);
  const ready = useAppStore((s) => s.ready);
  const init = useAppStore((s) => s.init);

  // 用 hash 记住当前页，刷新与深链接（含截图核对）都直接落到同一页。
  useEffect(() => {
    window.location.hash = `/${page}`;
  }, [page]);

  useEffect(() => {
    const onHashChange = () => setPage(pageFromHash());
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  useEffect(() => {
    void init();
  }, [init]);

  useEffect(() => {
    let disposed = false;
    let unsubscribers: Array<() => void> = [];

    void (async () => {
      const next = await Promise.all([
        subscribe<TrafficPayload>("nano:traffic", (payload) =>
          useAppStore.getState().applyTraffic(payload)
        ),
        subscribe<ProxyStatus>("nano:proxy", (status) =>
          useAppStore.getState().applyStatus(status)
        ),
        subscribe<RulesSummary>("nano:rules", (summary) => {
          useAppStore.getState().applyRules(summary);
          useRuleStore.getState().applySummary(summary);
        }),
        subscribe<RemoteOutcome>("nano:remote", (outcome) =>
          useRuleStore.getState().applyRemote(outcome)
        ),
      ]);
      if (disposed) next.forEach((off) => off());
      else unsubscribers = next;
    })();

    return () => {
      disposed = true;
      unsubscribers.forEach((off) => off());
    };
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key === ",") {
        event.preventDefault();
        setPage("settings");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-base">
      <div
        className="pointer-events-none fixed inset-0 -z-10 bg-[radial-gradient(1100px_600px_at_85%_-10%,rgba(10,132,255,0.10),transparent_60%)]"
        aria-hidden
      />
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <NavRail active={page} onChange={setPage} />
        <main className="min-w-0 flex-1 overflow-y-auto">
          {!ready ? (
            <LoadingScreen />
          ) : (
            <>
              {page === "home" && <HomePage onNavigate={setPage} />}
              {page === "nodes" && <NodesPage />}
              {page === "rules" && <RulesPage />}
              {page === "settings" && <SettingsPage />}
            </>
          )}
        </main>
      </div>
      <Toaster />
    </div>
  );
}
