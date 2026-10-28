/**
 * 本地一键发版：版本号 → 提交打 tag → 推送 → 建 Release。
 * 安装包编译与资产上传交给 GitHub Actions（`.github/workflows/release.yml`）。
 *
 *   bun run release                     # 自动递增 patch
 *   bun run release --version 0.3.0     # 指定版本号（必须大于当前基线）
 *   bun run release --no-release        # 只更新版本号并推送，不打 tag、不建 Release
 *   bun run release --yes               # 跳过确认
 *
 * 默认流程：校验环境 → 算版本号 → 同步清单 → 提交 + 打 tag → 推送 main 与 tag
 *          → 建 Release（发布态）→ CI 的 plan 转 draft 并打包 → finalize 自动发布。
 *
 * 操作说明见 RELEASE.md「1. 本地一键发布」，实现分散在 scripts/release/ 下：
 * lib.mjs（基础件）· publish.mjs（preflight、提交推送、建 Release）。
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline/promises";

import {
  BUN,
  MAIN,
  ROOT,
  capture,
  compareVersion,
  fail,
  log,
  parseVersion,
  run,
  succeeds,
} from "./release/lib.mjs";
import { commitAndPush, createRelease, preflight } from "./release/publish.mjs";

const SET_VERSION = join(ROOT, "scripts", "set-version.mjs");

const USAGE = `用法：bun run release [选项]

选项：
  --version <x.y.z>   指定版本号；必须严格大于「package.json 版本」与「最高 tag」
  --no-release        只更新版本号、提交并推送；不打 tag、不建 Release
  --yes, -y           跳过发布前确认
  --help, -h          显示本帮助

不带 --version 时自动把基线版本号的 patch +1。
安装包与更新清单由 GitHub Actions 编译上传，本机不需要签名私钥。`;

function parseArgs(argv) {
  const opts = { version: null, noRelease: false, yes: false, help: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help" || arg === "-h") opts.help = true;
    else if (arg === "--no-release") opts.noRelease = true;
    else if (arg === "--yes" || arg === "-y") opts.yes = true;
    else if (arg === "--version") {
      const value = argv[i + 1];
      if (!value || value.startsWith("-")) fail("--version 后面要跟版本号，例如 --version 1.2.3");
      opts.version = value;
      i += 1;
    } else if (arg.startsWith("--version=")) {
      opts.version = arg.slice("--version=".length);
    } else {
      fail(`未知参数：${arg}（用 --help 查看用法）`);
    }
  }
  return opts;
}

/** 基线 = package.json 版本与所有 v* tag 里最大的那个（手动打过高 tag 也不会被冲回去）。 */
function versionBase() {
  const pkgVersion = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version;
  const tags = capture("git", ["tag", "--list", "v*"])
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);

  const candidates = [pkgVersion, ...tags].map(parseVersion).filter(Boolean);
  const base = candidates.length
    ? candidates.reduce((best, item) => (compareVersion(item, best) >= 0 ? item : best))
    : { text: "0.0.0", core: [0, 0, 0], pre: null };
  const highestTag = tags.length ? `v${base.text}` : "无";
  return { version: base, source: `package.json ${pkgVersion} / 最高 tag ${highestTag}` };
}

function resolveTargetVersion(opts, base) {
  if (!opts.version) {
    return parseVersion(`${base.core[0]}.${base.core[1]}.${base.core[2] + 1}`);
  }
  const wanted = parseVersion(opts.version);
  if (!wanted) fail(`版本号格式不对：「${opts.version}」。期望形如 0.2.0 或 v0.2.0`);
  if (compareVersion(wanted, base) <= 0) {
    fail(`版本号只能递增：${wanted.text} 不大于当前基线 ${base.text}，请给一个更大的版本号`);
  }
  return wanted;
}

async function confirm(question) {
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  const answer = await rl.question(`${question} [y/N] `);
  rl.close();
  if (!/^y(es)?$/i.test(answer.trim())) fail("已取消，未做任何改动");
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) {
    log(USAGE);
    return;
  }
  // 子进程用 process.execPath 拉起 bun，用 node 跑会拉错解释器。
  if (!process.versions.bun) fail("请用 bun 运行：bun run release");

  const repo = preflight(opts);
  const base = versionBase();
  const target = resolveTargetVersion(opts, base.version);
  const version = target.text;
  const tag = `v${version}`;

  log(`\n版本：${base.version.text} → ${tag}（基线：${base.source}）`);
  log(
    opts.noRelease
      ? "流程：同步清单 → 提交 → 推送 main（不打 tag、不建 Release）"
      : `流程：同步清单 → 提交 + tag ${tag} → 推送 → 建 Release（${repo}），打包由 GitHub Actions 完成`
  );

  if (!opts.yes) await confirm("\n继续？");

  log("\n▶ 同步版本号…");
  run(BUN, [SET_VERSION, version]);

  if (opts.noRelease) {
    commitAndPush({ tag, message: `chore: bump version to ${tag}`, withTag: false });
    log(`\n✔ 版本号已更新为 ${tag} 并推送到 origin/${MAIN}（未打 tag、未建 Release）`);
    return;
  }

  if (capture("git", ["tag", "--list", tag])) fail(`tag ${tag} 已存在，换一个版本号`);
  if (succeeds("gh", ["release", "view", tag])) fail(`Release ${tag} 已存在，换一个版本号`);

  log("\n▶ 提交版本号并打 tag…");
  commitAndPush({ tag, message: `chore: release ${tag}`, withTag: true });

  log("\n▶ 创建 Release…");
  createRelease({ tag, version, repo });
}

await main();
