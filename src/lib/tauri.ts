/**
 * 与 Tauri 运行时交互的薄封装。
 * 在普通浏览器（vite dev / 截图核对）中运行时不存在的 API 会安全降级，
 * 保证前端可独立调试。
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";

export const isTauri: boolean =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export interface AppErrorShape {
  code: string;
  message: string;
}

export class AppError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "AppError";
    this.code = code;
  }
}

const CODE_HINTS: Record<string, string> = {
  not_running: "代理尚未启动，请先开启连接",
  no_node: "请先在节点页添加并选择一个节点",
  node_not_found: "该节点已不存在，请刷新列表",
  port_in_use: "端口被占用，换一个端口或关闭占用程序",
  port_permission: "该端口需要管理员权限，请使用 1024 以上的端口",
  invalid_uuid: "UUID 格式不正确",
  unsupported_config: "该节点配置暂不支持",
  upstream_unreachable: "无法连接节点服务器，请检查地址与网络",
  rejected: "该请求已被规则拒绝",
};

function normalizeError(err: unknown): AppError {
  if (err instanceof AppError) return err;
  if (typeof err === "object" && err !== null) {
    const shape = err as Partial<AppErrorShape>;
    if (typeof shape.code === "string" && typeof shape.message === "string") {
      return new AppError(shape.code, shape.message);
    }
  }
  if (typeof err === "string") {
    const [code, ...rest] = err.split(":");
    if (rest.length > 0 && CODE_HINTS[code]) {
      return new AppError(code, rest.join(":").trim());
    }
    return new AppError("unknown", err);
  }
  return new AppError("unknown", "操作失败，请重试");
}

export function errorMessage(err: unknown): string {
  const normalized = normalizeError(err);
  if (normalized.message && normalized.message !== normalized.code) {
    return normalized.message;
  }
  return CODE_HINTS[normalized.code] ?? "操作失败，请重试";
}

export async function invoke<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  if (!isTauri) {
    throw new AppError("preview", "浏览器预览模式，无法执行该操作");
  }
  try {
    return await tauriInvoke<T>(command, args);
  } catch (err) {
    throw normalizeError(err);
  }
}

export async function subscribe<T>(
  event: string,
  handler: (payload: T) => void
): Promise<UnlistenFn> {
  if (!isTauri) return () => undefined;
  return tauriListen<T>(event, (e) => handler(e.payload));
}

async function currentWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

/** 运行时版本号，来自构建时写入的 tauri.conf.json，避免 UI 里再硬编码一份。 */
export async function appVersion(): Promise<string> {
  if (!isTauri) return "preview";
  try {
    const { getVersion } = await import("@tauri-apps/api/app");
    return await getVersion();
  } catch {
    return "unknown";
  }
}

export async function minimizeWindow(): Promise<void> {
  if (!isTauri) return;
  await (await currentWindow()).minimize();
}

export async function toggleMaximizeWindow(): Promise<boolean> {
  if (!isTauri) return false;
  const win = await currentWindow();
  await win.toggleMaximize();
  return win.isMaximized();
}

export async function closeWindow(): Promise<void> {
  if (!isTauri) return;
  await (await currentWindow()).close();
}

export async function startDragging(): Promise<void> {
  if (!isTauri) return;
  await (await currentWindow()).startDragging();
}
