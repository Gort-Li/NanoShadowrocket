/** git / gh 相关：环境检查、提交与推送、创建 Release（打包交给 Actions）。 */
import { MAIN, MANIFESTS, capture, fail, log, parseVersion, run, succeeds } from "./lib.mjs";

/**
 * 发布前置检查。返回 GitHub 仓库全名（`--no-release` 时返回 null）。
 *
 * 要求干净工作区与最新的 origin/main：发版提交不能被别的改动污染，
 * 也不能和远端分叉。
 */
export function preflight(opts) {
  if (!succeeds("git", ["rev-parse", "--is-inside-work-tree"])) fail("当前目录不是 git 仓库");

  const branch = capture("git", ["rev-parse", "--abbrev-ref", "HEAD"]);
  if (branch !== MAIN) fail(`当前分支是 ${branch}，发布只允许在 ${MAIN} 上进行`);

  if (capture("git", ["status", "--porcelain"])) {
    fail("工作区有未提交的改动，先提交或 stash 再发版（版本号提交必须干净）");
  }
  if (!succeeds("git", ["remote", "get-url", "origin"])) fail("没有名为 origin 的远程仓库");

  log("▶ 同步远程状态…");
  run("git", ["fetch", "origin", MAIN, "--tags", "--prune"], { stdio: "ignore" });
  const behind = Number(capture("git", ["rev-list", "--count", `HEAD..origin/${MAIN}`]));
  if (behind > 0) fail(`本地落后 origin/${MAIN} ${behind} 个提交，先 git pull 再发版`);

  // --no-release 只用 git push，不需要解析 GitHub 仓库。
  if (opts.noRelease) return null;

  if (!succeeds("gh", ["auth", "status"])) fail("GitHub CLI 未登录，先执行 gh auth login");
  const repo = capture("gh", ["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"]);
  if (!repo) fail("无法解析 GitHub 仓库（gh repo view 失败）");
  return repo;
}

/** 提交四个版本清单，可选打 tag，然后推送。 */
export function commitAndPush({ tag, message, withTag }) {
  // 提交只能包含四个版本清单：其它文件有改动说明工作区状态不对，先停下。
  const unexpected = capture("git", ["status", "--porcelain"])
    .split(/\r?\n/)
    .filter(Boolean)
    .filter((line) => !MANIFESTS.some((file) => line.endsWith(file)));
  if (unexpected.length > 0) {
    fail(`出现计划外的改动，先确认再发版：\n${unexpected.map((line) => `  ${line}`).join("\n")}`);
  }

  run("git", ["add", ...MANIFESTS]);
  if (!capture("git", ["diff", "--cached", "--name-only"])) {
    fail("清单文件没有变化，版本号已经是目标值，无需提交");
  }
  run("git", ["commit", "-m", message]);
  if (withTag) run("git", ["tag", "-a", tag, "-m", `Release ${tag}`]);
  run("git", ["push", "origin", `HEAD:${MAIN}`]);
  if (withTag) run("git", ["push", "origin", tag]);
}

/**
 * 建 Release 并发布，触发 `release.yml` 打包。
 *
 * 这里**不**上传任何资产：`plan` 发现缺资产会先把 Release 转回 draft，
 * 由 `build` 编译并上传 `setup.exe` / `.sig` / `latest.json`，`finalize` 再发布。
 */
export function createRelease({ tag, version, repo }) {
  if (succeeds("gh", ["release", "view", tag])) fail(`Release ${tag} 已存在，换一个版本号或先删除它`);

  const args = ["release", "create", tag, "--title", tag, "--generate-notes"];
  // 预发布不占用「最新版」，正式版由 GitHub 按发布时间自动标 latest。
  if (parseVersion(version).pre) args.push("--prerelease");

  if (!succeeds("gh", args)) fail(`创建 Release ${tag} 失败。提交与 tag 已推送，修好后可在网页上手动建`);

  log(`\n✔ 已创建 Release ${tag}：https://github.com/${repo}/releases/tag/${tag}`);
  log(`  GitHub Actions 正在打包并发布资产：https://github.com/${repo}/actions/workflows/release.yml`);
  log("  构建完成后 Release 会自动补全安装包与更新清单，失败则停在 draft。");
}
