# 发版手册

面向维护者。日常发版只需要点几下 Actions，剩下的由工作流完成。

## 0. 一次性准备

### 生成更新签名密钥

应用内更新要求安装包带 minisign 签名。首次发版前生成一对密钥：

```powershell
New-Item -ItemType Directory -Force "$env:USERPROFILE\.tauri" | Out-Null
bun run tauri signer generate -w "$env:USERPROFILE\.tauri\nano-shadowrocket.key"
```

> 如果目标文件已存在，CLI 会拒绝覆盖。先确认旧密钥是否还在使用；确实要换新密钥时，先备份再删除旧文件，或改用另一个路径。

- 命令会提示设置密码；直接回车两次表示不设密码（本项目当前按无密码配置）。
- 私钥：`$env:USERPROFILE\.tauri\nano-shadowrocket.key`，**不要提交、不要外传**。
- 公钥：`$env:USERPROFILE\.tauri\nano-shadowrocket.key.pub`，内容是 base64 字符串；可以公开，但需要提交到仓库供客户端校验。

把公钥文件的**完整内容**（只复制 base64 那一行，不要带换行）粘贴到 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`：

```json
"updater": {
  "pubkey": "这里粘贴 .key.pub 的完整内容",
  "endpoints": ["..."]
}
```

> `pubkey` 不能写文件路径，必须是 `.key.pub` 里的 base64 文本。
> 如果仓库里已经有一把旧公钥，而你要换新密钥，必须同时替换这里的值；否则客户端会拒绝新包。
> 已有可用密钥时不要重复生成：换密钥会让老客户端无法自动更新。

### GitHub Secrets

仓库 **Settings → Secrets and variables → Actions** 里需要两个 Secret：

| 名称 | 内容 |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | 私钥文件的**完整内容**（`$env:USERPROFILE\.tauri\nano-shadowrocket.key` 整段文本） |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 生成密钥时设置的密码；没有密码就留空 |

复制到剪贴板（不要粘到聊天或提交到仓库）：

```powershell
# 私钥：粘贴到 GitHub Secret
Get-Content "$env:USERPROFILE\.tauri\nano-shadowrocket.key" -Raw | Set-Clipboard
# 公钥：粘贴到 tauri.conf.json
(Get-Content "$env:USERPROFILE\.tauri\nano-shadowrocket.key.pub" -Raw).Trim() | Set-Clipboard
```

> **私钥丢了就等于永远发不了更新。**
> 老客户端只认这把公钥，换一把密钥后它们再也无法自动更新，只能引导用户手动下载安装包。
> 请把私钥文件复制到密码管理器或离线磁盘，至少留一份备份。

### 密钥文件

```text
私钥：%USERPROFILE%\.tauri\nano-shadowrocket.key        # 不要提交到仓库
公钥：%USERPROFILE%\.tauri\nano-shadowrocket.key.pub    # 内容填进 tauri.conf.json
```

重新生成密钥会作废旧的更新链，除非同时保留旧私钥，否则不要动它。

## 1. 发版

三条路，任选一条。**版本号永远来自 tag**，工作流会把它同步到 `package.json` / `Cargo.toml` / `tauri.conf.json` / `Cargo.lock`。

### 方式 A：手动建 Release（最省事，推荐日常用）

1. **Releases → Draft a new release**
2. tag 填 `v0.2.0`（必须 `vX.Y.Z`），标题随便填，正文可写可不写
3. 点 **Save draft** 就行——**不用再去点 Actions**
4. 剩下的全自动：15 分钟内 `Release Watch` 捡到它 → 构建安装包 → 上传 `latest.json` → 补全 Release 正文 → 发布

要点：

- **草稿不会触发任何 workflow**。这是 GitHub 的硬限制（官方文档：draft release 的 `created` / `edited` / `deleted` 事件不启动工作流），所以只能靠 `Release Watch` 每 15 分钟轮询一次。想立刻开始，去 **Actions → Release Watch → Run workflow**。
- **直接点 Publish 也安全**。工作流发现缺资产会先把 Release 转回 draft，构建完再发布，客户端不会看到「最新版却没有 `latest.json`」的半成品。
- **正文里写的东西会保留**，自动生成的「下载 / 变更 / 校验」三段追加在后面。唯一例外：你在 draft 里点过 **Generate release notes**，那部分会被重新生成的内容替换（避免重复）。
- **不想让它自动构建**（例如只想挂个说明页）：正文里加一行 `<!-- no-auto-build -->`，`Release Watch` 会跳过它。
- tag 不合 `vX.Y.Z` 会直接报错退出，不会瞎猜版本号。

### 方式 B：Bump Version 自动算版本号

1. **Actions → Bump Version → Run workflow**
2. `level` 选 `patch` / `minor` / `major`，勾 `dry_run` 可以只看算出来的版本号
3. 工作流会：算出下一个版本号 → 同步三处清单与 `Cargo.lock` → 提交并打 tag → 创建 **draft** Release → 显式 dispatch `Release`

### 方式 C：手动 dispatch Release

**Actions → Release → Run workflow**，填 tag（必须已经有对应的 Release）。用于重跑失败的构建，或 `force` 强制重建已有资产。

### 版本号规则

- 语义化版本 `MAJOR.MINOR.PATCH`，tag 形如 `v0.2.0`；预发布可写 `v0.3.0-beta.1`
- 常规修复走 `patch`，新功能走 `minor`，破坏性改动才动 `major`
- 递增基数取「`package.json` 版本」与「仓库最高 tag」中的较大者，所以手动打了更高的 tag 也不会被 bump 冲回去
- 预发布（prerelease）不会占用 `releases/latest`，客户端检查更新时看不到它

## 2. 工作流做了什么

### Release Watch（`release-watch.yml`）

每 15 分钟扫一遍仓库所有 Release（**含 draft**），挑出「tag 合法 + 14 天内创建 + 缺 `latest.json` 或 `setup.exe` + 没有 `no-auto-build` 标记」的，逐个 dispatch `Release`。同一 tag 已经有排队/运行中的构建就跳过；失败 2 次后停止自动重试，需要人工介入。

### Release（`release.yml`）

拆成三段，任何一段失败都不会把半成品推给客户端：

1. **plan**：解析 tag（事件里的 `release.tag_name`，或 dispatch 的输入）→ 校验格式 → 查 Release 是否存在 → 判断资产是否已齐 → 如果已公开但缺资产，先转回 draft
2. **build**：按 tag 检出 → `bun install` → `prepare:sidecar` → `set-version` → `tauri-action` 编译 NSIS 并上传 `NanoShadowrocket_<版本>_x64-setup.exe`、`.sig`、`latest.json`
3. **finalize**：校验 `latest.json`（必须有 `windows-x86_64` 条目、版本与 tag 一致、指向的 `.exe` 资产真实存在）→ 重写正文（保留手写内容 + generate-notes 变更列表 + 下载/校验说明）→ 发布 draft（预发布标 prerelease，正式版标 latest）

资产已齐时 `plan` 会把 `need_build` 置 false，`build` 整段跳过，`finalize` 照常收尾。所以重复触发、事件与轮询撞车都不会重复编译——`concurrency` 按 tag 串行，后一个跑起来时资产已经齐了。

## 3. 本地构建

开启 `bundle.createUpdaterArtifacts` 后，本地出安装包也需要签名密钥：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "$env:USERPROFILE\.tauri\nano-shadowrocket.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""   # 密钥有密码时填密码
bun run tauri build
```

日常开发用 `bun run tauri:dev`，不需要密钥。两种命令都会先执行 `prepare:sidecar`；如果下载失败，可以单独重跑：

```bash
bun run prepare:sidecar
```

改了版本号但忘了同步三处清单或 `Cargo.lock` 时，CI 会发现不一致（以 `package.json` 为基准）：

```bash
bun scripts/set-version.mjs 0.2.0            # 写入
bun scripts/set-version.mjs --check           # 只校验三处清单与 Cargo.lock
bun scripts/set-version.mjs 0.2.0 --check     # 指定期望版本校验
```

## 4. 故障排查

| 现象 | 原因与处理 |
| --- | --- |
| 建好 draft 后一直没开始构建 | 最长等 15 分钟（轮询间隔）。急着发就 **Actions → Release Watch → Run workflow** 手动扫一次。仍不动就看：tag 是否 `vX.Y.Z`、正文有没有 `no-auto-build`、`Release Watch` 是否被 GitHub 自动停用（仓库 60 天无活动会禁用 schedule） |
| Release 被自动转回了 draft | 正常行为：原本是发布状态但缺资产，工作流先转回 draft 避免 `releases/latest` 指向半成品。构建成功后会自动重新发布 |
| 失败后 `Release Watch` 不再自动重试 | 同一 tag 失败 2 次后停止自动重试，避免每 15 分钟烧一次 CI。修好后 **Actions → Release → Run workflow** 手动跑一次 |
| 构建报 `Signing key not found` | Secret 没配或私钥内容不完整；检查 `TAURI_SIGNING_PRIVATE_KEY` 是否包含整段私钥 |
| 构建报 `No updater private key password provided` | 密钥没有密码时的正常提示；如果密钥有密码，检查 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 是否配置 |
| 工作流卡在 `校验更新清单` | `latest.json` 没生成、版本不匹配或资产缺失，看 tauri-action 日志里有没有 `Updater asset ... not found` |
| Bump 创建了 Release 但没有开始构建 | 检查 Bump 的 `触发 Release 构建` 步骤；需要 `actions: write`，且 `release.yml` 必须已在默认分支上。即便 dispatch 失败，`Release Watch` 也会在 15 分钟内补触发 |
| 构建失败后客户端仍检查旧版本 | 预期行为：Release 还是 draft，`releases/latest` 不会指向它；修好后重跑 Release 工作流即可 |
| 客户端提示已是最新版本但确实有新版 | `latest.json` 的 `version` 取自 `tauri.conf.json`，检查 tag 与清单版本是否同步 |
| 客户端下载后校验签名失败 | 私钥与 `pubkey` 不配对，确认用的是最初那把 |
| 构建报 `sidecar_error` / 找不到 `v2ray.exe` | 先跑 `bun run prepare:sidecar`；确认 `src-tauri/binaries/v2ray-x86_64-pc-windows-msvc.exe` 存在且 SHA256 未被改动 |
| `prepare:sidecar` 报 SHA256 不匹配 | 下载被代理或镜像篡改，删除 `src-tauri/binaries/.v2ray-windows-64.zip` 后重试 |
| bump 工作流推送失败 | `main` 分支被保护，把仓库管理员加进允许推送的白名单，或临时取消保护 |
| 客户端提示已是最新版本但确实有新版 | `latest.json` 的 `version` 取自 `tauri.conf.json`，检查 tag 与清单版本是否同步 |
| 客户端下载后校验签名失败 | 私钥与 `pubkey` 不配对，确认用的是最初那把 |
| 构建报 `sidecar_error` / 找不到 `v2ray.exe` | 先跑 `bun run prepare:sidecar`；确认 `src-tauri/binaries/v2ray-x86_64-pc-windows-msvc.exe` 存在且 SHA256 未被改动 |
| `prepare:sidecar` 报 SHA256 不匹配 | 下载被代理或镜像篡改，删除 `src-tauri/binaries/.v2ray-windows-64.zip` 后重试 |
| bump 工作流推送失败 | `main` 分支被保护，把仓库管理员加进允许推送的白名单，或临时取消保护 |
