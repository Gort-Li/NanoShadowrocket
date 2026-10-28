# AGENTS.md

> 给接手这个仓库的 AI / 开发者的工程手册。
> 面向用户的安装、使用与常见问题在 [README.md](README.md)。

## 0. 硬性约定

1. **包管理与脚本一律用 bun**：`bun install`、`bun run build`、`bun run tauri build`。不要生成 `package-lock.json`，不要用 npm / npx。
2. **不 panic**：命令层与网络层返回 `Result`，错误经 `AppError` 转成 `{ code, message }`。
3. **不泄漏敏感信息**：日志与错误文案里禁止出现节点 UUID、订阅 URL 的查询参数。新增日志前先确认。
4. **改行为就补测试**：规则匹配、VMess 编解码、配置迁移属于必须单测的区域。
5. **改 UI 就重截截图**：README 引用 `docs/screenshots/*.png`，视觉变化后同步更新（见第 12 节）。
6. **单文件尽量 300 行内**，超了就按职责拆模块；不要引入新的重型依赖。

## 1. 项目是什么

Windows 桌面代理客户端：单端口混合入站（HTTP + SOCKS5）→ 规则分流 → VMess（内置 v2ray-core sidecar）或直连。
界面用 React，入站 / 规则 / 统计 / 生命周期在 Rust，全部跑在 Tauri 的 tokio 运行时里。
VMess 的协议细节（AEAD、legacy `alterId`、UDP）交给随安装包分发的 v2ray-core sidecar；
Rust 只通过本地 SOCKS5 使用它，**没有 TUN、没有系统服务**。

明确不做：Electron、TUN/wintun、系统服务、把 Go 运行时嵌入主进程。
sidecar 只作为出站，不接管系统流量；当前 UI 只暴露 `tcp` 传输，ws / grpc / TLS 仍未开放。

## 2. 工具链与命令

| 场景 | 命令 |
| --- | --- |
| 安装依赖 | `bun install` |
| 下载 / 校验 v2ray sidecar | `bun run prepare:sidecar` |
| 前端开发（浏览器，自动演示数据） | `bun run dev` |
| 桌面端开发（推荐） | `bun run tauri:dev` |
| 前端类型检查 + 构建 | `bun run build` |
| 打包 NSIS | `bun run tauri:build` |
| 本地一键发版（版本号 + tag + 推送 + 建 Release，打包由 CI 完成） | `bun run release [--version X.Y.Z] [--no-release]` |
| 后端格式化 | `cd src-tauri && cargo fmt` |
| 静态检查（必须 0 警告） | `cargo clippy --lib --tests` |
| 单元测试 | `cargo test --lib` |
| 同步版本号 | `bun scripts/set-version.mjs 0.2.0` |
| 原生 VMess 真实互操作测试 | `cargo test --test vmess_e2e -- --ignored --test-threads=1` |
| sidecar 真实互操作测试（TCP + UDP + legacy alterId） | `cargo test --test sidecar_e2e -- --ignored --test-threads=1` |

版本基线：Rust 1.90+、Tauri 2.12、React 19、Vite 5、Tailwind 3.4、bun 1.4。

## 3. 目录地图

```
.github/workflows/          ci / release 两个工作流
scripts/                    set-version.mjs、release.mjs（本地一键发版：版本号 + tag + Release）、fetch-v2ray.mjs（sidecar 下载校验）、shot-rules.cjs
RELEASE.md                  发版手册（面向维护者的操作步骤，原理在本文件第 11 节）

src/                         前端
├── App.tsx                  页面切换（hash 路由）、事件订阅、快捷键
├── pages/                   四个页面：Home / Nodes / Rules / Settings
├── components/              TitleBar、NavRail、Sheet、ConfirmDialog、RuleEditor、RuleSheet、ActiveConnections…
├── store/useAppStore.ts     连接、流量、活动连接、节点、设置、toast
├── store/useRuleStore.ts    自定义规则、订阅列表、规则测试、一键规则
├── lib/tauri.ts             invoke / listen 封装、错误归一化、窗口操作
├── lib/types.ts             与后端 serde 一一对应的类型
├── lib/mock.ts              浏览器预览用的演示数据（含已连接状态与活动连接）
├── lib/active.ts            主域名启发式、活动连接聚合与策略排序
├── lib/customRules.ts       自定义规则文本 ↔ 表单模型、校验、upsert
└── lib/format.ts            速率、流量、时间、策略标签

src-tauri/src/               后端
├── lib.rs                   启动、单实例、托盘、1s 流量 ticker、60s 订阅调度
├── state.rs                 AppState：配置 + 规则集 + 运行中的入站 + 测速 + 订阅
├── commands/                Tauri 命令（mod / nodes / rules）
├── events.rs                事件常量与推送
├── error.rs                 AppError 与错误码
├── config/                  AppConfig、原子落盘、vmess:// 解析
├── rules/                   解析器、匹配索引、GEOIP、远程订阅
├── proxy/                   入站、路由、出站、DNS、统计
│   ├── socks5_client.rs     连本地 sidecar 的 SOCKS5 客户端（CONNECT / UDP）
│   ├── udp.rs               UdpOutbound / UdpFlow：直连与 sidecar UDP
│   └── inbound/udp.rs       SOCKS5 UDP ASSOCIATE 服务端
├── sidecar/                 v2ray-core 配置生成、启动/就绪/退出、Job Object
├── vmess/                   原生 AEAD 实现（保留用于参考与 vmess_e2e）
├── sysproxy/                系统代理快照/写入/还原、代理环境变量、开机自启
└── resources/               内置 CN 网段

src-tauri/binaries/          构建时下载的 v2ray.exe（gitignore，不提交）
src-tauri/resources/         随安装包分发的 v2ray-LICENSE.txt
src-tauri/tests/sidecar_e2e.rs  真实 v2ray-core 的 legacy TCP + UDP 测试（ignored）
```

## 4. 运行架构

### 启动时序（`lib.rs`）

1. `AppState::new(app_config_dir)`：加载 `config.json` → 若 `proxy_dirty` / `env_dirty` 为真则立即还原 → 清理上次残留的 sidecar 配置 → 编译规则集 → 创建 `RouterHandle`。
2. `app.manage(Arc<AppState>)`。
3. `setup_tray`：托盘菜单 id 为 `show` / `toggle` / `quit`。
4. `spawn_traffic`：每秒 `stats.sample()`，把 `stats`、`running` 与 `active.snapshot()` 打包成 `TrafficPayload` 并 `emit("nano:traffic")`。
5. `spawn_remote_scheduler`：每 60 秒检查一次到期订阅，到期就更新并 `emit` 事件。
6. `RunEvent::ExitRequested` 时同步调用 `state.restore_all()`；sidecar 由 Job Object 在进程退出时一并结束。

### 连接生命周期（`proxy/inbound`）

```
TcpStream ──peek 首字节──> 0x05 走 SOCKS5，否则走 HTTP
   → Accepted { Target, preface, kind }
   → RouterHandle.get().decide(target)   // 可能触发 DNS
   → 非 Reject 时 active.begin(...) 返回 RAII guard
   → Policy::Proxy ? SidecarOutbound : DirectOutbound
   → reply_success()
   → 先写 preface（HTTP 明文请求 / CONNECT 多读的字节）
   → 双向 copy，边转发边累加 TrafficStats
```

- HTTP 只支持 `CONNECT host:port` 与绝对 URI（`GET http://host/path`），每连接一个请求，不复用。
- SOCKS5 接受 no-auth + `CONNECT` 与 `UDP ASSOCIATE`；节点关闭 UDP 时 `UDP ASSOCIATE` 回复 `0x07`。
- `UDP ASSOCIATE` 的 TCP 控制连接决定关联生命周期；第一个数据报的源地址会被固定，避免被当作开放 UDP 中继。
- HTTP 入站不承载 UDP；UDP 只对显式配置 SOCKS5 的程序生效，系统代理不转发 UDP。
- 拒绝策略返回 HTTP 403 / SOCKS5 `0x02`；上游失败返回 502 / `0x05`。
- 活动连接由 `proxy/stats.rs` 的 `ActiveConnections` 维护：TCP 每条连接一个 guard，UDP 按
  `host:port` 去重并在关联结束（guard drop）时移除；拒绝的连接不进入列表。

### v2ray sidecar（`src-tauri/src/sidecar`）

- 所有 VMess 出站都走 sidecar：`AppState::start` 为当前节点生成最小配置，启动
  `v2ray.exe run -c <config_dir>/v2ray-sidecar.json`，只监听 `127.0.0.1:<随机端口>` 的 SOCKS 入站（`udp: true`）。
- TCP 用 `proxy/socks5_client.rs` 的 `CONNECT`；UDP 用 `UDP ASSOCIATE` + 一条 `UdpFlow`。
- `V2raySidecar` 在 `AppState::stop` 时 drop：`Child::kill` + 删除含 UUID 的临时配置。
- Windows Job Object 设置 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，应用崩溃或被强制结束时 sidecar 不会残留。
- 子进程用 `CREATE_NO_WINDOW` 启动，stdout/stderr 写到 `v2ray-sidecar.log`；启动失败时把日志尾部拼进 `sidecar_error`。
- 配置里 `alterId > 0` 由 v2ray-core 使用 legacy 请求头；`aes-*-cfb` 映射为 `security: "auto"`（JSON 配置没有 `legacy` 选项）。
- 入站、规则、统计仍在 Rust 侧，sidecar 只负责 VMess 编解码与 UDP 转发。

### 规则热替换

`AppState.router` 是 `Arc<RouterHandle>`，内部 `RwLock<Arc<Router>>`；入站持有句柄，每个新连接重新取。
修改自定义规则、订阅、默认策略、DNS 时调用 `AppState::reload_rules()` 重建 `Router` 并整体替换，
**已建立的连接不受影响**。路由决策有 120 秒 TTL、上限 1 万条的域名缓存，随 Router 一起丢弃。
`update_settings` 只在端口 / 局域网 / 系统代理 / 环境变量变化时重启入站；默认策略、DNS、IPv6
只热替换 Router。前端会先乐观合并设置，再以后端返回的 `ProxyStatus` 与 `nano:rules` 校正。

### 事件

| 事件 | 负载 | 触发 |
| --- | --- | --- |
| `nano:traffic` | `TrafficPayload { stats, running, active }` | 每秒 |
| `nano:proxy` | `ProxyStatus` | 启停、设置变更、切换节点 |
| `nano:rules` | `RulesSummary` | 规则重编译 |
| `nano:remote` | `RemoteOutcome` | 单个订阅更新完成 |

前端在 `App.tsx` 统一订阅；组件只从 store 读，不直接 listen。

## 5. 数据与 IPC 契约

### 配置文件

- 位置：`app.path().app_config_dir()`，即 `%APPDATA%\com.nanoshadowrocket.desktop\config.json`。
- 规则缓存：同目录 `rules/<订阅 id>.conf`。
- sidecar 临时文件：同目录 `v2ray-sidecar.json`（含 UUID，正常停止即删除）与 `v2ray-sidecar.log`（保留最近一次启动日志）。
- 写入方式：临时文件 + 原子 rename（`config/store.rs`）；解析失败会把原文件备份成 `config.json.bak` 并回退默认值。
- 字段全部 `camelCase`（`#[serde(rename_all = "camelCase", default)]`），前端 `lib/types.ts` 必须同步。
- `NodeEntry.udp` 默认 `true`；旧配置没有该字段时视为允许 UDP，旧链接的 `udp=0` 才会关闭。
- `Bootstrap` 与 `TrafficPayload` 的 `active` 是 `ActiveConnection[]`（`id/host/port/policy/rule/startedAt/protocol`）；
  `protocol` 序列化为小写 `tcp` / `udp`。前端 `useAppStore.active` 直接保存该数组，不再有 `recent` 字段。

### 命令清单

| 分组 | 命令 |
| --- | --- |
| 启动 | `bootstrap`、`get_status`、`get_traffic` |
| 连接 | `start_proxy`、`stop_proxy`、`toggle_proxy` |
| 设置 | `update_settings`、`clear_rule_cache`、`reset_config` |
| 节点 | `preview_vmess_link`、`add_node_from_link`、`save_node`、`delete_node`、`select_node`、`test_node_latency`、`test_all_latencies` |
| 规则 | `save_custom_rules`、`rules_summary`、`list_remote_rules`、`add_remote_rule`、`update_remote_rule`、`update_all_remote_rules`、`remove_remote_rule`、`set_remote_interval`、`set_remote_enabled`、`reorder_remote_rules`、`test_rule` |

新增命令必须同时改三处：`commands/*.rs`、`lib.rs` 的 `generate_handler!`、`src/lib/tauri.ts` 的调用方。

### 错误码

`AppError` 序列化为 `{ code, message }`，`message` 已经是中文可读文案，前端直接用，不要二次拼接。

| code | 含义 |
| --- | --- |
| `port_in_use` / `port_permission` | 端口被占用 / 需要管理员权限（<1024） |
| `no_node` / `node_not_found` | 未选节点 / 节点已删除 |
| `invalid_input` | 参数校验失败（UUID、URL、端口等） |
| `unsupported_config` | 节点用了未实现的传输或加密 |
| `sidecar_error` | 内置 v2ray 内核缺失、启动失败、握手失败或提前退出 |
| `invalid_uuid` / `upstream_unreachable` / `rejected` | 来自 `ProxyError` |
| `remote_error` / `config_error` / `io_error` / `internal_error` | 订阅、配置、文件、内部错误 |

## 6. 规则引擎（`src-tauri/src/rules`）

### 解析（`parser.rs`）

- 逐行流式解析，只关心 `[Rule]` 与 `[General]`；其它段直接跳过。
- 支持：`DOMAIN`、`DOMAIN-SUFFIX`、`DOMAIN-KEYWORD`、`IP-CIDR`（v4/v6）、`GEOIP`、`FINAL`。
- 策略归一化：`proxy|allow`、`direct|bypass`、`reject|deny|block`（大小写不敏感）。
- 非法行不报错，写入 `ignored` 并把原因聚合进 `ignored_by_reason`，UI 显示「已忽略 N 条」。

### 编译顺序（`build_ruleset`）

`sources` 的顺序就是优先级：自定义规则 → 各订阅（用户拖动后的顺序）→ 内置直连（`BUILTIN_DIRECT`：localhost、`.local/.lan/.internal`、回环/私网/链路本地/ULA）→ 各来源的 `FINAL` → 默认策略。
同一来源内，`DOMAIN` 精确规则优先于后缀规则；不同来源之间先看来源顺序。

### 匹配结构（`matcher.rs`）

- `DOMAIN`：`HashMap<String, usize>`
- `DOMAIN-SUFFIX`：`HashMap<String, usize>`，查询时枚举域名的各级后缀
- `DOMAIN-KEYWORD`：Aho-Corasick 一次扫描
- `IP-CIDR`：排序区间 + 二分
- `GEOIP`：内置 CN 网段编译成前缀树

### 真实订阅的实测基线

`tests/rules_real.rs` 会下载 Johnshall 的两个 top500 规则集并验证解析、编译与典型域名：

| 订阅 | 生效条数 | 编译耗时 | 忽略 |
| --- | --- | --- | --- |
| `sr_top500_banlist_ad.conf`（3.5MB） | 91,248（FINAL 计入） | ~0.38s | 1 条 `RULE-SET` |
| `sr_top500_whitelist_ad.conf`（2.4MB） | 61,778 | ~0.29s | 1 条 `RULE-SET` |

- `RULE-SET`（外部列表引用）不在支持范围内，遇到会忽略并计入 `ignored_by_reason`。
- 规则集自带的 `[General]`（`dns-server` / `skip-proxy` / `bypass-tun`）只解析不使用，
  不要指望订阅能改变程序的 DNS 或内网直连策略。
- 白名单版的 `GEOIP,CN,DIRECT` 会让未命中域名规则的域名进入 `needs_resolve`；
  解析失败时 `Router::decide` 回退到域名层结果，不会丢连接。
- 多个来源都带 `FINAL` 时只保留优先级最高的一条（`build_ruleset` 的 `final_rule`），
  因此黑白名单不能同时启用；需要在文档或 UI 中继续强调这一点。
- 统计口径：`RuleStats.custom / remote` 在编译时累加，`rules_summary()` 直接读取；
  不要为了展示条数去重新 `parse_conf` 几 MB 的订阅文本。

### DNS 触发条件

只有「当前已命中的规则之后，仍存在可能改变结果的 IP/GEOIP 规则」且未标 `no-resolve` 时才解析；
解析结果按 TTL 缓存在 `proxy/dns.rs`。DNS 支持 `system` 与 DoH JSON 端点（`https://...`），
`ipv6 = false` 时只取 A 记录。

## 7. VMess：sidecar 为生产路径

- **生产路径**：`AppState::start` 启动 v2ray-core sidecar，所有 VMess 节点（`alterId = 0` 与 `> 0`）都经本地 SOCKS5 出站。
- `alterId = 0`：v2ray-core 使用 AEAD 请求头；`alterId > 0`：legacy 请求头 + alterId 派生认证。
- UDP：sidecar 的 SOCKS 入站开启 `udp: true`；NanoShadowrocket 通过 `UDP ASSOCIATE` 转发，节点 `udp = false` 时拒绝。
- 传输：sidecar 配置目前固定 `network: "tcp"`、`security: "none"`；`Transport::supported()` 只允许 tcp，ws / grpc / TLS 仍未接入。
- 加密：`CipherMethod::Aes128Gcm | ChaCha20Poly1305` 直接映射；`Legacy`（`aes-128-cfb` / `aes-256-cfb`）映射为 `"auto"`，由 alterId 决定实际头格式。
- sidecar 配置不写 routing / DNS / sniffing；域名原样交给 v2ray-core，直连 UDP 的域名由 Rust 侧 `Router::resolve` 解析。
- `src-tauri/src/vmess/` 的原生 AEAD 实现保留：
  - 用于 `tests/vmess_e2e.rs` 与协议回归；
  - `AppState` 生产路径不再使用 `VmessOutbound`（单测通过 `native_outbound` 开关保留 start/stop 覆盖）。
- 原生实现仍只支持 AEAD + TCP；`alterId != 0` 会返回 `unsupported_config`。新增传输不要改 `chunk.rs` / `header.rs`，应扩展 sidecar 配置或新增 `Outbound` 实现。
- **响应头惰性**是原生实现与官方 v2ray-core 互通的关键；改动 `vmess/transport.rs` 前先跑 `tests/vmess_e2e.rs`。

## 8. 系统代理与开机自启（`src-tauri/src/sysproxy`）

- 只读写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` 的
  `ProxyEnable`、`ProxyServer`、`ProxyOverride`、`AutoConfigURL`（PAC 会临时移除，还原时写回）。
- 写入顺序必须是：**先成功 bind 入站 → 再写注册表**；写失败则关闭入站并回滚 `proxy_dirty`。
  系统代理与环境变量由 `AppState::apply_host_integration` 统一写入，任一步失败都走 `restore_all` 回滚。
- 四条还原路径：手动停止（`AppState::stop`）、应用退出（`ExitRequested` 调 `restore_all`）、
  崩溃残留（下次启动 `AppState::new` 检测 `proxy_dirty`）、**卸载**
  （NSIS `PREUNINSTALL` 钩子执行 `NanoShadowrocket.exe --restore-proxy`）。
- 还原是**无条件**的，不提供开关。早期版本的 `restoreProxyOnExit` 已删除（UI 上是摆设，
  后端从未读取），老配置里的残留字段由 serde 静默忽略，不要重新引入这个字段。
- 写入后调用 `InternetSetOptionW(SETTINGS_CHANGED / REFRESH)`，失败不影响注册表结果。
- 开机自启写 `HKCU\...\CurrentVersion\Run` 的 `NanoShadowrocket` 值，命令行带 `--hidden`。
- 主窗口在 `tauri.conf.json` 里是 `visible: false`，由 `setup` 决定是否 `show()`：
  静默启动（`--hidden`）永不显示，只驻留托盘；正常启动才显示。不要改回 `visible: true`，
  否则开机自启会先闪一个空窗口再被 `hide()`。
- `AppConfig.auto_start_proxy`（默认 true）为真时，静默启动会按 2 / 10 / 30 秒退避自动
  `state.start()`，失败只写日志，不弹窗。
- 单实例由 `tauri-plugin-single-instance` 保证，避免多进程反复改系统代理。

### 代理环境变量（`sysproxy/env_proxy.rs`）

- 目的：`curl` / `git` / `npm` / `ssh` 不读 `Internet Settings`，只认环境变量。
- 写 `HKCU\Environment` 的 `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` / `NO_PROXY`，
  然后广播 `SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, ..., "Environment")`，
  **否则新开的终端拿不到新值**（explorer 不会主动重读注册表）。
- 只写注册表，不要用 `std::env::set_var`：那只影响当前进程，对用户毫无意义。
- 写入前快照 `EnvValue { value, expand }`，`expand` 用于区分 `REG_SZ` 与 `REG_EXPAND_SZ`，
  还原时必须按原类型写回。快照里没有的变量代表原本不存在，还原要删除。
- `env_dirty` / `env_snapshot` 与 `proxy_dirty` 同构，四条还原路径完全一致。
- 平台限制要在 UI 里讲清楚：只对新开的终端生效。

### 卸载还原（`src-tauri/windows/nsis-hooks.nsh`）

- `tauri.conf.json` 的 `bundle.windows.nsis.installerHooks` 指向该文件，
  只定义 `NSIS_HOOK_PREUNINSTALL`，用 `ExecWait` 调 `--restore-proxy`。
- `main.rs` 在启动 GUI 之前拦截该参数，`lib.rs::restore_proxy_from_disk` 用
  `tauri::generate_context!()` 取 identifier 拼出 `%APPDATA%\{identifier}`，
  不硬编码路径（宏展开的 `Context` 无法推断运行时，必须显式写 `Context<Wry>`）。
- 卸载时程序可能已被强制结束，所以这条路径**不能**依赖内存里的 `AppState`。
- 改 exe 名（`mainBinaryName`）时要同步钩子里的两个 `IfFileExists` 分支。

## 9. 前端约定

- **store 分工**：`useAppStore` 管连接、流量、节点、设置；`useRuleStore` 管规则文本、订阅、测试。
  两者有循环引用，只在函数体内互相调用，不要在模块顶层访问。
- **预览模式**：`lib/tauri.ts` 用 `__TAURI_INTERNALS__` 判断是否在 Tauri 内。非 Tauri 时 `invoke` 抛
  `AppError("preview")`，`init()` 改用 `lib/mock.ts` 的演示数据（默认已连接），便于浏览器调试与截图。
- **标题栏拖动**：只用 `data-tauri-drag-region`。Tauri 2.12 的 `drag.js` 已经在 `document` 上接管
  mousedown（自带双击最大化、自动跳过 `button` 等可点击元素），所以**不要**再写
  `onMouseDown → startDragging()`：二次调用会走到 tao 的 `handle_os_dragging`，那里的
  `ReleaseCapture()` 会打断正在进行的那次拖动，表现为偶发「拖不动」。
- **遮罩会盖住标题栏**：`Sheet`（`z-50`）/ `ConfirmDialog`（`z-60`）的 `fixed inset-0` 遮罩在
  标题栏（`z-40`）之上，打开期间窗口拖不动是当前的设计选择；要让抽屉打开时也能拖窗口，
  需要把遮罩改成从标题栏下方开始（只有 38px 的高度差，注意与 `TitleBar` 的 `h-[38px]` 对齐）。
- **Portal**：`Sheet` / `ConfirmDialog` 必须 `createPortal(..., document.body)`。页面根节点有
  `animate-fade-up`（带 transform），会让子级 `position: fixed` 相对页面而不是视口定位。
- **路由**：无 react-router，用 `location.hash`（`#/home`、`#/nodes`、`#/rules`、`#/settings`）并监听 `hashchange`。
- **样式**：颜色 token 在 `tailwind.config.js`（`base/surface/elevated/ink/accent/ok/danger/warn/info`）。
  自定义 rgba 颜色不要再叠加 `/opacity` 修饰符（如 `bg-accent-soft/50` 会得到意料之外的亮度）；
  需要透明度时直接用任意值（`bg-[rgba(...)]` / `bg-white/[0.04]`）。
- **交互**：所有可点击元素必须是 `<button>`；开关用 `Switch`（role=switch）；表格/列表行高稳定，避免 hover 时布局跳动。
- **Switch**：轨道用 `inline-flex items-center p-[2px]`，滑块是普通 flex 子项，靠 `translate-x-[18px]` 移动；
  不要再改成绝对定位 + 静态位置，否则父级 `text-align: center` 会让滑块溢出轨道。
- **首页顶部卡片**：不再有独立页面标题；大圆按钮就是连接状态，节点名后面紧跟可复制的本地地址，
  运行时长胶囊与节点名同一行、靠右。不要把状态标题或地址重复塞回页面头部。
- **活动连接**：`lib/active.ts` 用常见多级后缀启发式求主域名（完整 PSL 不引入）；分组策略取子项最高优先级
  （proxy → direct → reject），同组内按连接数、域名排序。`ActiveConnections` 同时只允许一个主域名展开。
  展开行的端口、协议、策略要紧跟在主机名后面，右侧只放一键规则按钮。
- **一键规则**：主域名用 `DOMAIN-SUFFIX`，具体主机用 `DOMAIN`，IP 用 `IP-CIDR`（v4 `/32`、v6 `/128`）；
  写入走 `useRuleStore.applyQuickRule`，按 `kind + value` upsert 到列表最前并立即保存。
- **自定义规则表单**：`lib/customRules.ts` 负责文本 ↔ 表单模型；注释在重写时丢弃，无法识别的原始行放入
  `unsupported` 并在保存时原样保留。新增规则与一键规则 upsert 到顶部，编辑保持原位置。
- **节点字段**：`Node` / `NodeInput` 都有 `udp: boolean`，`NodeSheet` 用 `Switch` 控制；`Security` 额外包含 `"legacy"`，对应旧链接的 `aes-*-cfb`。
- **响应式**：断点用 `max-[860px]:`（窗口最窄 780px），主卡在窄窗口改为纵向堆叠。

## 10. 测试

- 单测跟模块放在一起（`#[cfg(test)] mod tests`），覆盖：规则解析/匹配、VMess AEAD 与分块、
  `vmess://` 解析、配置往返、错误码、SOCKS5 握手、UDP 包编解码、sidecar 配置生成、HTTP 头解析、统计速率、
  活动连接的 guard 自动移除与清空、默认策略热替换。
- `tests/vmess_e2e.rs` 需要真实 v2ray-core（`127.0.0.1:10086`）与 `python -m http.server 18080`，
  默认 `#[ignore]`。注意 `#[ignore = "..."]` 的值必须是**字面量**，不能用常量。
- `tests/sidecar_e2e.rs` 自行拉起本地 v2ray-core 服务端，验证 `alterId = 0` AEAD、`alterId = 64` legacy TCP、
  SOCKS5 UDP ASSOCIATE，以及完整 `AppState` 入站链路；需要先 `bun run prepare:sidecar`，默认 `#[ignore]`。
- `tests/rules_real.rs` 需要访问 GitHub Pages，默认 `#[ignore]`，用于验证大订阅与真实规则集：
  `cargo test --test rules_real -- --ignored --nocapture`。
- 提交前至少跑：

```bash
bun run build
cd src-tauri && cargo fmt && cargo clippy --lib --tests && cargo test --lib
```

## 11. 构建与发布

- `src-tauri/Cargo.toml` 的 `[profile.release]` **关闭了 LTO**（`lto = false`、`codegen-units = 16`）。
  原因：fat LTO + 单 codegen unit 让每次链接多花约 100 秒，而安装包只小 0.2MB。
- 首次打包需完整编译 release 依赖（本机约 8-10 分钟），之后每次约 1-2 分钟；瓶颈在磁盘/杀软扫描。
  把 `src-tauri/target` 加入 Windows Defender 排除项可显著提速。
- 产物：`src-tauri/target/release/nano-shadowrocket.exe`、
  `src-tauri/target/release/bundle/nsis/NanoShadowrocket_<version>_x64-setup.exe`。
- sidecar 通过 `tauri.conf.json` 的 `bundle.externalBin: ["binaries/v2ray"]` 打进安装包；
  源码文件名必须是 `src-tauri/binaries/v2ray-x86_64-pc-windows-msvc.exe`，安装后位于主程序同目录的 `v2ray.exe`。
- `scripts/fetch-v2ray.mjs` 固定 v2ray-core `v5.53.0`，同时校验 zip 与 exe 的 SHA256；
  `beforeDevCommand` / `beforeBuildCommand` 会自动执行 `bun run prepare:sidecar`。
- 安装包因此增加约 15-20MB（压缩后）；`v2ray.exe` 本身约 40MB，不提交仓库。
- 应用标识符 `com.nanoshadowrocket.desktop` 一经发布不要改，否则配置目录与升级链会断。

### 版本号体系

- **唯一来源是 git tag**（`vX.Y.Z`），不要手改任何一处版本号。
- 三处清单文件与 `Cargo.lock` 中的本包版本由 `scripts/set-version.mjs` 同步：
  `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.lock`。
  `TitleBar.tsx` 不再硬编码，改为运行时读 `getVersion()`。
- `bun scripts/set-version.mjs 0.2.0` 写入；`--check` 只校验，不一致时退出 1。
  不带版本号时以 `package.json` 为基准，CI 用这个模式检查三处清单与 `Cargo.lock` 是否同步。
- 新增需要记录版本号的地方时，**不要**再写死字符串。

### 发布入口

只有两个入口，**共用同一个工作流** `release.yml`，不存在第二套发布流程。打包（NSIS 安装包 +
minisign 签名 + `latest.json`）与 Release 收尾一律由它完成，本地脚本只改版本号、打 tag、
建 Release，不编译、不上传资产：

| 入口 | 本机做的事 | 谁打包 |
| --- | --- | --- |
| 本地一键发版 `bun run release` | 改版本号、提交、打 tag、推送、建 Release | GitHub Actions |
| GitHub 建 Release | 网页上打 tag + 建 Release 并 **Publish** | GitHub Actions |

- 两条路的共同点：都靠「建一个发布态 Release」触发同一条 `release: published` 事件，
  因此 CI 侧不需要算版本号 / 提交 / 打 tag 的能力。历史上的 `bump.yml`（Actions 里选
  patch/minor/major）与 `release.yml` 的 `workflow_dispatch` 入口已删除：本地脚本覆盖了它们，
  而它们都要额外处理「`GITHUB_TOKEN` 创建的 Release 不触发 `release` 事件」这条限制（只能靠
  dispatch 补），属于纯复杂度。
- **draft Release 的 `created` / `edited` 不触发任何 workflow**（GitHub 官方限制）。
  所以「建 Release 就自动发版」的前提是**点 Publish 而不是 Save draft**；停在草稿什么都不会发生。
  不要为此加轮询工作流：15 分钟一次的 cron 在私有仓库一个月烧 ~2880 计费分钟，不值。
- **重试不靠 dispatch**：CI 构建失败时 Release 停在 draft，重新点一次 Publish 会再次触发同一事件。

### 本地发布脚本（`scripts/release.mjs`）

- 入口 `release.mjs` 负责 CLI、版本基线与编排；`scripts/release/lib.mjs`（路径 / 子进程 / semver）
  与 `publish.mjs`（preflight、提交推送、建 Release）。
- 步骤：校验 → 算版本号 → 同步清单 → 提交 → 打 annotated tag → 推送 `main` 与 tag
  → `gh release create --generate-notes`（预发布加 `--prerelease`）。
- 版本基线取 max(`package.json` 版本, 最高 `v*` tag)：手动打过高 tag 不会被冲回去。
  不带 `--version` 时 patch +1；带了必须严格大于基线，否则报错退出。
- **提交前只允许四个版本清单是脏的**，其它改动一律中止，防止把杂项混进发版提交。
- **脚本不编译、不上传资产**：`gh release create` 只建一个没有资产的发布态 Release，
  `release.yml` 的 `plan` 发现缺资产会先转回 draft，再由 `build` 编译上传、`finalize` 发布。
  `latest.json` 由 `tauri-action` 的 `uploadUpdaterJson` 生成（签名字段来自 `.sig`），本地不要手写。
- **建完 Release 就返回，不轮询构建状态**：剩下的交给 `release.yml`，用户不必等打包结束。
- `--no-release` 只做「同步清单 → 提交 → 推送」，不打 tag、不建 Release。
- 子进程用 `process.execPath` 拉起 bun，因此脚本必须用 bun 运行（脚本里有运行时检查）。

### GitHub Actions（`.github/workflows`）

| 工作流 | 触发 | 作用 |
| --- | --- | --- |
| `ci.yml` | push main / PR | 版本号一致性、前端构建、`cargo fmt --check`、`clippy -D warnings`、`cargo test --lib` |
| `release.yml` | `release: published` | plan（解析 tag、查资产、必要时转回 draft）→ build（按 tag 同步版本号并打包上传）→ finalize（校验 `latest.json` → 重写正文 → 发布） |

- `release.yml` 幂等：资产已齐时 `need_build=false` 跳过整个 build，只做校验与发布；
  `concurrency` 按 tag 串行，重复发布不会并发打包。
- 两条入口建出来的都是「已发布但还没有资产」的 Release，`plan` 会先把它转回 draft，
  资产齐了才重新发布。因此构建期间 `releases/latest` 仍指向上一个完整版本；中间有几秒
  「已发布但缺 `latest.json`」的窗口，客户端最多报一次检查失败，重试即恢复。
- 打包用 `tauri-apps/tauri-action@v0`，`releaseId` 指向已存在的 Release，
  `uploadUpdaterJson: true` 生成 `latest.json`，`updaterJsonPreferNsis: true` 确保 Windows
  更新指向 NSIS 的 `setup.exe`。`releaseDraft` 必须显式传入当前 draft 状态——它默认 `false`，
  会把 draft 提前公开。
- `latest.json` 的 `platforms["windows-x86_64"]` 指向 `setup.exe` 资产本身（不是 zip），
  客户端 `tauri-plugin-updater` 能直接识别 exe 并以 NSIS 参数运行。
- `finalize` 没有源码需求但会大量调用 `gh`。`gh` 在无 `-R`/`GH_REPO` 时会退回到本地 git 检测仓库，
  因此 `finalize` 先 `actions/checkout@v4` 建立 `.git` 与 origin，所有 `gh release` 命令都显式传
  `-R "$REPO"`，从根上避免 `failed to run git: not a git repository`。
- 面向维护者的操作步骤见 [RELEASE.md](RELEASE.md)。

### 应用内更新

- 依赖 `tauri-plugin-updater`；`tauri.conf.json` 里 `bundle.createUpdaterArtifacts: true`
  与 `plugins.updater.{ pubkey, endpoints }`。`pubkey` 是 minisign 公钥，随应用分发。
- endpoint 固定为 `https://github.com/Gort-Li/NanoShadowrocket/releases/latest/download/latest.json`。
- 前端逻辑在 `src/lib/update.ts`（check / download / install），状态在 `useAppStore` 的 `update` 字段，
  UI 在设置页的「关于」组。启动 6 秒后静默检查一次。
- Windows 上 NSIS 安装器会替换 exe 并自行重启应用，**不需要** `tauri-plugin-process` 的 `relaunch`。
- 先下载更新包，下载完成后再 `stop_proxy` 并安装；避免下载失败时把用户断网，也避免安装器替换 exe 时端口仍被占用。
- 私钥只存在于 GitHub Secret 与本地 `~/.tauri/`，**禁止**进仓库；丢了就发不了更新。

### 截图更新

```bash
bun run dev
# 另开终端，用 Playwright/无头 Chromium 截图（deviceScaleFactor: 2）
# 首页 900x1000（要完整展示活动连接树），节点页 900x760
# 规则页 900x880，设置页 900x900（内容更长）
# 规则表单截图在规则页打开「添加规则」抽屉后按 900x880 截
# 规则测试页 900x1200（表单 + 订阅 + 测试结果完整展示）
# 覆盖 docs/screenshots/{home,nodes,rules,settings,node-form,rule-form,rule-test}.png
```

截图依赖 `lib/mock.ts` 的演示数据；修改演示节点或订阅后记得同步重截。

## 12. 常见任务

**新增一种规则类型**

1. `rules/parser.rs`：加 `RuleKind` 分支、解析与校验、`label()`/`value()`。
2. `rules/matcher.rs`：建索引、加入 `decide_domain` / `decide_ip` 的候选比较与 `needs_resolve` 判断。
3. `lib/customRules.ts` 的 `CustomRuleKind` / 解析 / 校验 / 序列化，以及 `RuleSheet.tsx` 的 `KINDS` 选项。
4. 补解析与匹配单测。

**新增一个命令**

1. 在 `commands/` 对应模块写 `#[tauri::command]`，参数结构体用 `#[serde(rename_all = "camelCase")]`。
2. `lib.rs` 的 `generate_handler!` 注册。
3. `src/lib/types.ts` + `src/lib/tauri.ts` 封装，页面通过 store 调用。
4. 返回 `Result<T, AppError>`；Tauri 会把它序列化成 `{ code, message }`。

**新增页面**

`components/NavRail.tsx` 的 `ITEMS` → `App.tsx` 的 `PAGES` 与渲染分支 → `pages/XxxPage.tsx`。

**换/加出站类型**

TCP 实现 `proxy::Outbound`（`async fn connect(&self, &Target) -> Result<BoxedStream>`）；
UDP 实现 `proxy::udp::UdpOutbound` + `UdpFlow`。在 `AppState::start` 里按配置选择，路由与 UI 不需要改。
VMess 的协议差异优先通过 sidecar 配置解决，不要把新传输写进 `vmess/chunk.rs`。

**新增 VMess 传输（ws / tls / grpc）**

1. `config/model.rs` 的 `Transport::supported()` 放开对应枚举。
2. `sidecar/config.rs` 补 `streamSettings`（path、host、TLS 证书等）；节点模型需要新增字段时同步 `NodeInput` 与前端表单。
3. `commands/nodes.rs` 的导入警告与校验同步。
4. 补 sidecar 配置单测，并用真实服务端跑 `tests/sidecar_e2e.rs`。

## 13. 已知陷阱

- **fixed 与 transform**：见第 9 节 Portal，抽屉/确认框都必须挂到 body。
- **Tauri 命令参数名**：前端 `invoke("save_node", { input })` 的 key 必须与 Rust 形参名一致；
  结构体内部字段走 camelCase。
- **`parking_lot::Mutex` 不能跨 `.await` 持有**；涉及 IO 的锁用 `tokio::sync::Mutex`（`AppState.runtime`）。
- **启动顺序**：先 bind 再改系统代理，否则端口占用时用户会被断网。
- **规则顺序**：`compile_sources` 里 custom 必须排第一；拖动排序只影响订阅之间。
- **`#[serde(default)]`**：新增配置字段必须给默认值，否则老配置文件会解析失败（虽然有 `.bak` 兜底）。
- **`Policy` 的序列化是小写**（`proxy/direct/reject`），前端 `POLICY_LABEL` 依赖这个。
- **端口 0**：测试里用 `listen_port = 0` 让系统分配，`ProxyStatus.local_port` 返回实际端口。
- **不要打印 UUID**：`sanitize` 只用于订阅 URL；节点相关日志只写名称与 `host:port`。
  sidecar 配置文件含 UUID，属于必要文件，正常停止会删除；不要把它的内容写进日志或错误文案。
- **sidecar 路径**：运行时只找主程序同目录的 `v2ray.exe`；开发态回退到
  `src-tauri/binaries/v2ray-x86_64-pc-windows-msvc.exe`。打包缺文件会直接报 `sidecar_error`。
- **UDP 只走 SOCKS5**：系统代理（WinINET）不承载 UDP；HTTP 入站也没有 UDP。
- **sidecar 生命周期**：必须依赖 `V2raySidecar` 的 `Drop` 或 Job Object，不要在命令层直接 `Child::kill`；
  否则含 UUID 的临时配置可能残留。
- **测试开关**：`AppState.native_outbound` 只在 `#[cfg(test)]` 下存在，生产路径不能被它绕过。
- **活动连接 guard**：`ActiveConnections::begin` 返回的 guard 必须在连接/关联任务里活到结束，不要 `let _ = ...`
  立即丢弃；UDP 用 `HashMap<String, ConnectionGuard>` 按目标去重，关联结束统一 drop。
- **一键规则顺序**：`applyQuickRule` 必须 upsert 到自定义规则最前，否则会被更宽的旧规则抢先命中；
  不要改成追加到末尾。
- **表单与原始规则**：`parseCustomRules` 会丢弃注释，但必须把 `unsupported` 原样写回；不要为了“清理”
  把无法识别的行删掉，否则用户会静默丢规则。

## 14. 代码风格

- Rust：`rustfmt` 默认风格；`thiserror` 定义错误；`log::{info,warn,debug}`，不要 `println!`；
  公共 API 写一行文档注释，模块头部写职责与边界。
- TypeScript：组件 PascalCase、工具函数 camelCase；避免 `any`；格式化/纯函数放 `lib/`；
  组件只做渲染，业务状态进 store。
- 注释只解释「为什么」，不复述代码。