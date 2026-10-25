/**
 * 把语义化版本号写入三处清单文件与 Cargo.lock 的本包条目。
 *
 * 版本号的唯一来源是 git tag（`vX.Y.Z`）。CI 在构建前调用本脚本把它同步到
 * package.json / tauri.conf.json / Cargo.toml / Cargo.lock，本地手工发版同理，
 * 避免多处手改漏掉一处。
 *
 *   bun scripts/set-version.mjs 0.2.0
 *   bun scripts/set-version.mjs v0.2.0 --check   # 只校验，不一致时退出码 1
 */
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

const args = process.argv.slice(2);
const checkOnly = args.includes("--check");
let raw = args.find((arg) => !arg.startsWith("--")) ?? process.env.RELEASE_TAG ?? "";

// CI 只做一致性检查时没有 tag 可传，用 package.json 当前版本作为基准。
if (!raw && checkOnly) {
  raw = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version ?? "";
}

const version = raw.trim().replace(/^v/i, "");

if (!SEMVER.test(version)) {
  console.error(`版本号无效：「${raw}」。期望形如 1.2.3 或 v1.2.3`);
  process.exit(1);
}

const TARGETS = [
  {
    file: "package.json",
    // package.json 里只有顶层一个 version 键。
    pattern: /("version"\s*:\s*")[^"]*(")/,
  },
  {
    file: "src-tauri/tauri.conf.json",
    pattern: /("version"\s*:\s*")[^"]*(")/,
  },
  {
    file: "src-tauri/Cargo.toml",
    // 限定在 [package] 段内，避免匹配到依赖项的 version 字段。
    pattern: /(\[package\][^[]*?\nversion\s*=\s*")[^"]*(")/,
  },
  {
    file: "src-tauri/Cargo.lock",
    // 只改本包条目，依赖项的 version 不能动；兼容 CI 上 checkout 出的 CRLF。
    pattern: /(name = "nano-shadowrocket"\r?\nversion = ")[^"]*(")/,
  },
];

let changed = 0;
const mismatched = [];

for (const { file, pattern } of TARGETS) {
  const abs = join(ROOT, file);
  const source = readFileSync(abs, "utf8");
  const match = source.match(pattern);

  if (!match) {
    console.error(`在 ${file} 中找不到版本号字段，无法写入`);
    process.exit(1);
  }

  const current = source.slice(
    match.index + match[1].length,
    match.index + match[0].length - match[2].length
  );

  if (current === version) continue;

  if (checkOnly) {
    mismatched.push(`${file}: ${current} ≠ ${version}`);
    continue;
  }

  writeFileSync(abs, source.replace(pattern, `$1${version}$2`), "utf8");
  changed += 1;
  console.log(`${file}: ${current} → ${version}`);
}

if (checkOnly) {
  if (mismatched.length > 0) {
    console.error(`版本号不一致（期望 ${version}）：\n${mismatched.join("\n")}`);
    process.exit(1);
  }
  console.log(`版本号一致：${version}`);
} else {
  console.log(changed > 0 ? `版本号已同步为 ${version}` : `版本号已是 ${version}，无需修改`);
}
