/**
 * 应用内更新：检查 → 下载 → 交给 NSIS 安装器。
 *
 * Windows 上安装器替换 exe 后会自行重启应用，因此这里不再调用 relaunch。
 * 浏览器预览模式直接视为「没有更新」，方便前端独立调试。
 */

import type { Update } from "@tauri-apps/plugin-updater";
import { isTauri } from "./tauri";

export interface UpdateInfo {
  version: string;
  notes: string | null;
}

export interface InstallProgress {
  /** 0-100；服务端没给 Content-Length 时保持 0。 */
  percent: number;
  downloaded: number;
  total: number | null;
}

/** `check()` 返回的句柄必须留着，下载时要用同一个对象。 */
let pending: Update | null = null;

function infoOf(update: Update): UpdateInfo {
  return {
    version: update.version,
    notes: update.body?.trim() || null,
  };
}

/** 检查更新。已经检查过且未安装时复用上次的句柄，避免重复请求与资源泄漏。 */
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  if (!isTauri) return null;
  if (pending) return infoOf(pending);

  const { check } = await import("@tauri-apps/plugin-updater");
  pending = await check();
  return pending ? infoOf(pending) : null;
}

/** 只下载更新包；下载完成后再停代理并安装，避免下载失败把用户断网。 */
export async function downloadPendingUpdate(
  onProgress: (progress: InstallProgress) => void
): Promise<void> {
  if (!pending) throw new Error("没有待安装的更新，请先检查更新");

  let total: number | null = null;
  let downloaded = 0;

  await pending.download((event) => {
    if (event.event === "Started") {
      total = event.data.contentLength ?? null;
      onProgress({ percent: 0, downloaded, total });
      return;
    }
    if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
      onProgress({
        percent: total ? Math.min(100, Math.round((downloaded / total) * 100)) : 0,
        downloaded,
        total,
      });
      return;
    }
    onProgress({ percent: 100, downloaded, total });
  });
}

/** 启动已下载的安装器。Windows 上进程会被安装器接管，本函数通常不再返回。 */
export async function installDownloadedUpdate(): Promise<void> {
  if (!pending) throw new Error("没有已下载的更新，请先检查更新");
  await pending.install();
}
