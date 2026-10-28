# 发版手册

面向维护者，只讲怎么操作。两条路任选一条，**安装包编译与资产上传统一由 GitHub Actions 完成**：

| 方式 | 什么时候用 | 入口 |
| --- | --- | --- |
| **本地一键发版** | 日常发版：本机改版本号、打 tag、建 Release | `bun run release` |
| **GitHub 建 Release** | 不在本机、或想完全在网页上操作 | Releases → Draft a new release → Publish |

## 0. 一次性准备

以下只影响 CI 打包，本机发版不需要私钥。

### 签名密钥

应用内更新要求安装包带 minisign 签名。首次发版前生成一对：

```powershell
New-Item -ItemType Directory -Force "$env:USERPROFILE\.tauri" | Out-Null
bun run tauri signer generate -w "$env:USERPROFILE\.tauri\nano-shadowrocket.key"
```

- 提示设置密码时直接回车两次表示不设密码（本项目按无密码配置）。
- 私钥：`%USERPROFILE%\.tauri\nano-shadowrocket.key`，不要提交、不要外传。
- 公钥：`%USERPROFILE%\.tauri\nano-shadowrocket.key.pub`，把里面的 base64 整行粘到
  `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。

> **私钥丢了就等于永远发不了更新。** 老客户端只认这把公钥，换密钥后它们只能手动下载安装包。

### GitHub Secrets

仓库 **Settings → Secrets and variables → Actions**：

| 名称 | 内容 |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | 私钥文件的完整内容 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 生成密钥时设的密码；没有就留空 |

```powershell
Get-Content "$env:USERPROFILE\.tauri\nano-shadowrocket.key" -Raw | Set-Clipboard
```

## 1. 本地一键发版

前置：在 `main` 分支、工作区干净、`gh auth login` 已登录。

```powershell
bun run release                  # 版本号 patch +1，打 tag 并建 Release
bun run release --version 1.3.0  # 指定版本号
bun run release --no-release     # 只更新版本号并推送：不打 tag、不建 Release
bun run release --yes            # 跳过确认
```

脚本按下面的顺序执行，任何一步失败都会停下并打印原因：

1. 校验分支、工作区、远程同步状态、`gh` 登录
2. 算出版本号（不传 `--version` 时 patch +1）
3. 同步 `package.json` / `Cargo.toml` / `Cargo.lock` / `tauri.conf.json`
4. 提交版本号、打 `vX.Y.Z` tag、推送 `main` 与 tag
5. 创建 Release（发布态）

然后交给 Actions：Release 工作流接手编译 NSIS 安装包，上传 `setup.exe` / `.sig` / `latest.json`，
校验通过后自动补全正文并发布。**脚本不等构建**，命令返回就可以关掉终端。

完成后：GitHub 的 Release 上能看到安装包，设置页的「软件更新」能检查到它。
进度看 **Actions → Release**：`https://github.com/<owner>/<repo>/actions/workflows/release.yml`。

注意事项：

- `--version` 必须大于当前版本，否则报错退出；预发布写 `1.3.0-beta.1`（不会占用「最新版」）。
- `--no-release` 只提交推送版本号，不打 tag、不建 Release。
- 版本号提交失败不会留下任何东西；推送成功但建 Release 失败时，去网页手动建同名 tag 的 Release 即可。
- 构建失败时 Release 停在 draft，重新点一次 Publish 就会重跑。

## 2. GitHub 建 Release

适合不在本机、或想完全在网页上操作时。

1. 建议先同步版本号：`bun run release --no-release`（否则仓库里的四处版本号会落后于 tag）
2. 打开 **Releases → Draft a new release**
3. tag 填 `vX.Y.Z`（必须 `vX.Y.Z` 格式，且未发布过；需与刚推的版本号一致），标题随便填
4. 点 **Publish release**（不要点 Save draft）
5. 等 **Actions → Release** 跑完：构建 → 上传资产 → 补全正文 → 发布

注意：

- **必须点 Publish**。停在草稿什么都不会发生。
- tag 格式不合法会直接失败，不会瞎猜版本号。
- 构建失败时 Release 会停在 draft，重新点一次 Publish 即可重试（不会影响已发布的旧版本）。

## 3. 故障排查

| 现象 | 处理 |
| --- | --- |
| `bun run release` 报「工作区有未提交的改动」 | 先提交或 stash，发版提交不能夹带别的改动 |
| `bun run release` 报「版本号只能递增」 | 当前版本号已经不低于目标值，换一个更大的版本 |
| `bun run release` 报「本地落后 origin/main」 | 先 `git pull` |
| 建好 Release 后什么都没发生 | 点的是 Save draft。改成点 **Publish release** |
| Release 被自动转回了 draft | 正常现象：发布时还没有资产，工作流先转回 draft 再构建，成功后自动重新发布；一直停在 draft 说明构建失败了，看构建日志 |
| 构建报 `Signing key not found` | `TAURI_SIGNING_PRIVATE_KEY` Secret 没配或内容不完整 |
| 构建报 `No updater private key password provided` | 密钥无密码时的正常提示；有密码就检查 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |
| 工作流卡在「校验更新清单」 | `latest.json` 缺失、版本与 tag 不一致或指向的资产没传上去，看 tauri-action 日志 |
| 客户端下载后校验签名失败 | 私钥与 `pubkey` 不配对，确认用的是最初那把 |
| 构建报 `sidecar_error` / 找不到 `v2ray.exe` | 工作流里 `prepare:sidecar` 失败，看该步骤日志；本地可先跑 `bun run prepare:sidecar` 复现 |
| `prepare:sidecar` 报 SHA256 不匹配 | 下载被代理或镜像篡改，删掉 `src-tauri/binaries/.v2ray-windows-64.zip` 后重试 |
| 推送失败（`main` 被保护） | 把仓库管理员加进允许推送的白名单，或临时取消保护 |
| 想手动验证打包结果 | 本机 `bun run tauri build`（需要设置 `TAURI_SIGNING_PRIVATE_KEY`，见 README「从源码构建」） |

发布链路的原理与实现细节在 [AGENTS.md](AGENTS.md) 第 11 节。
