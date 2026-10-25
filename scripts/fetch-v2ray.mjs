// 下载并校验固定版本的 v2ray-core，供 Tauri externalBin 打包。
// 只保留 v2ray.exe；geoip.dat / geosite.dat 不参与本项目的单出站配置。

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const VERSION = "v5.53.0";
const URL = `https://github.com/v2fly/v2ray-core/releases/download/${VERSION}/v2ray-windows-64.zip`;
const ZIP_SHA256 = "46ee170d031ea3be79cc583e61e6335a612e4c87925deed2a7179b0a5fb79f60";
const EXE_SHA256 = "e7a78aa95f164569658e8d1affb566c510be119e643b71cfab45be12de649cec";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binDir = join(root, "src-tauri", "binaries");
const target = join(binDir, "v2ray-x86_64-pc-windows-msvc.exe");
const zipPath = join(binDir, ".v2ray-windows-64.zip");
const tempDir = join(binDir, ".v2ray-extract");

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function ensureWindows() {
  if (process.platform !== "win32") {
    throw new Error("v2ray-core 目前只为 Windows x64 打包");
  }
}

async function download() {
  console.log(`[v2ray] 下载 ${VERSION} ...`);
  const response = await fetch(URL, {
    headers: { "User-Agent": "nano-shadowrocket-build" },
    redirect: "follow",
  });
  if (!response.ok) {
    throw new Error(`下载失败：HTTP ${response.status}`);
  }
  const buffer = Buffer.from(await response.arrayBuffer());
  mkdirSync(binDir, { recursive: true });
  writeFileSync(zipPath, buffer);
}

function extract() {
  rmSync(tempDir, { recursive: true, force: true });
  mkdirSync(tempDir, { recursive: true });
  const quote = (value) => `'${value.replaceAll("'", "''")}'`;
  execFileSync(
    "powershell",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `Expand-Archive -LiteralPath ${quote(zipPath)} -DestinationPath ${quote(tempDir)} -Force`,
    ],
    { stdio: "inherit" },
  );
  const extracted = join(tempDir, "v2ray.exe");
  if (!existsSync(extracted)) {
    throw new Error("压缩包里没有 v2ray.exe");
  }
  copyFileSync(extracted, target);
}

async function main() {
  ensureWindows();
  mkdirSync(binDir, { recursive: true });

  if (existsSync(target) && statSync(target).isFile()) {
    const actual = sha256(target);
    if (actual === EXE_SHA256) {
      console.log(`[v2ray] 已就绪：${target}`);
      return;
    }
    console.warn("[v2ray] 现有文件校验失败，重新下载");
  }

  await download();
  const zipHash = sha256(zipPath);
  if (zipHash !== ZIP_SHA256) {
    rmSync(zipPath, { force: true });
    throw new Error(`压缩包 SHA256 不匹配：${zipHash}`);
  }

  extract();
  const exeHash = sha256(target);
  if (exeHash !== EXE_SHA256) {
    rmSync(target, { force: true });
    throw new Error(`v2ray.exe SHA256 不匹配：${exeHash}`);
  }

  rmSync(zipPath, { force: true });
  rmSync(tempDir, { recursive: true, force: true });
  console.log(`[v2ray] 已写入 ${target}`);
}

main().catch((error) => {
  console.error(`[v2ray] ${error.message}`);
  process.exit(1);
});
