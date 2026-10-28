/** 发布脚本共用的基础件：路径、日志、子进程封装、语义化版本比较。 */
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
/** 用当前 bun 可执行文件拉起子进程，避免 PATH 上找不到 bun。 */
export const BUN = process.execPath;
export const MAIN = "main";
export const MANIFESTS = [
  "package.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
  "src-tauri/tauri.conf.json",
];

const VERSION_RE = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

export function log(message = "") {
  console.log(message);
}

export function fail(message) {
  console.error(`\n✖ ${message}`);
  process.exit(1);
}

/** 前台执行，输出直接透传；非 0 退出码就终止整个脚本。 */
export function run(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, { cwd: ROOT, stdio: "inherit", ...options });
  if (result.error) fail(`无法执行 ${cmd}：${result.error.message}`);
  if (result.status !== 0) fail(`${[cmd, ...args].join(" ")} 失败（退出码 ${result.status}）`);
  return result;
}

/** 静默执行并返回 stdout；失败直接终止。 */
export function capture(cmd, args) {
  const result = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8" });
  if (result.error) fail(`无法执行 ${cmd}：${result.error.message}`);
  if (result.status !== 0) {
    fail(`${[cmd, ...args].join(" ")} 失败：${(result.stderr || "").trim()}`);
  }
  return (result.stdout || "").trim();
}

/** 只看退出码，失败不终止。 */
export function succeeds(cmd, args) {
  const result = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8" });
  return !result.error && result.status === 0;
}

export function parseVersion(raw) {
  const text = String(raw ?? "").trim().replace(/^v/i, "");
  const match = VERSION_RE.exec(text);
  if (!match) return null;
  return {
    text,
    core: [Number(match[1]), Number(match[2]), Number(match[3])],
    pre: match[4] ?? null,
  };
}

/** 语义化比较：core 逐段比，相等时「正式版 > 预发布」。 */
export function compareVersion(a, b) {
  for (let i = 0; i < 3; i += 1) {
    if (a.core[i] !== b.core[i]) return a.core[i] > b.core[i] ? 1 : -1;
  }
  if (a.pre === b.pre) return 0;
  if (a.pre === null) return 1;
  if (b.pre === null) return -1;
  return a.pre > b.pre ? 1 : -1;
}
