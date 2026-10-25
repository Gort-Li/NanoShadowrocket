export function formatBytes(bytes: number, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value.toFixed(index === 0 ? 0 : digits)} ${units[index]}`;
}

export function formatRate(bytesPerSecond: number): { value: string; unit: string } {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) {
    return { value: "0", unit: "B/s" };
  }
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  const index = Math.min(
    Math.floor(Math.log(bytesPerSecond) / Math.log(1024)),
    units.length - 1
  );
  const value = bytesPerSecond / 1024 ** index;
  return {
    value: index === 0 ? String(Math.round(value)) : value.toFixed(1),
    unit: units[index],
  };
}

export function formatDuration(startedAt?: number | null): string {
  if (!startedAt) return "00:00:00";
  const seconds = Math.max(0, Math.floor(Date.now() / 1000 - startedAt));
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  return [h, m, s].map((n) => String(n).padStart(2, "0")).join(":");
}

export function formatRelativeTime(unix?: number | null): string {
  if (!unix) return "从未更新";
  const seconds = Math.floor(Date.now() / 1000 - unix);
  if (seconds < 60) return "刚刚";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟前`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} 小时前`;
  if (seconds < 86400 * 30) return `${Math.floor(seconds / 86400)} 天前`;
  const date = new Date(unix * 1000);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(
    date.getDate()
  ).padStart(2, "0")}`;
}

export function formatClock(unix: number): string {
  const date = new Date(unix * 1000);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(
    2,
    "0"
  )}:${String(date.getSeconds()).padStart(2, "0")}`;
}

export function latencyTone(ms?: number | null): "ok" | "warn" | "bad" | "none" {
  if (ms == null) return "none";
  if (ms < 120) return "ok";
  if (ms < 320) return "warn";
  return "bad";
}

export function formatLatency(ms?: number | null): string {
  if (ms == null) return "—";
  return `${ms} ms`;
}

export const POLICY_LABEL: Record<string, string> = {
  proxy: "代理",
  direct: "直连",
  reject: "拒绝",
};

export function policyTone(policy: string): string {
  if (policy === "reject") return "text-danger border-danger/30 bg-danger/10";
  if (policy === "direct") return "text-ink-muted border-white/10 bg-white/[0.04]";
  return "text-accent border-accent/30 bg-accent-soft";
}
