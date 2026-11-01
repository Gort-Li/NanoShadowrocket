// 下载并校验固定版本的 Xray-core，供 Tauri externalBin 打包。
// 只保留 xray.exe；geoip.dat / geosite.dat 不参与本项目的单出站配置。

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

const VERSION = "v26.10.31";
const URL = `https://github.com/XTLS/Xray-core/releases/download/${VERSION}/Xray-windows-64.zip`;
const ZIP_SHA256 = "2E04A9995A6F2AD1B28C0BB25C708ECB2695E43A1C9C7889C4B6AC2B48CF7A6A".toLowerCase();
const EXE_SHA256 = "D5B558191CCCB1E92A5772B68B8CAB67717B26F634B0F15153215F65AE982A7E".toLowerCase();

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binDir = join(root, "src-tauri", "binaries");
const target = join(binDir, "xray-x86_64-pc-windows-msvc.exe");
const zipPath = join(binDir, ".xray-windows-64.zip");
const tempDir = join(binDir, ".xray-extract");

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function ensureWindows() {
  if (process.platform !== "win32") {
    throw new Error("Xray-core 目前只为 Windows x64 打包");
  }
}

async function download() {
  console.log(`[xray] 下载 ${VERSION} ...`);
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
  const extracted = join(tempDir, "xray.exe");
  if (!existsSync(extracted)) {
    throw new Error("压缩包里没有 xray.exe");
  }
  copyFileSync(extracted, target);
}

async function main() {
  ensureWindows();
  mkdirSync(binDir, { recursive: true });

  if (existsSync(target) && statSync(target).isFile()) {
    const actual = sha256(target);
    if (actual === EXE_SHA256) {
      console.log(`[xray] 已就绪：${target}`);
      return;
    }
    console.warn("[xray] 现有文件校验失败，重新下载");
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
    throw new Error(`xray.exe SHA256 不匹配：${exeHash}`);
  }

  rmSync(zipPath, { force: true });
  rmSync(tempDir, { recursive: true, force: true });
  console.log(`[xray] 已写入 ${target}`);
}

main().catch((error) => {
  console.error(`[xray] ${error.message}`);
  process.exit(1);
});