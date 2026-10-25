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

Windows 桌面代理客户端：单端口混合入站（HTTP + SOCKS5）→ 规则分流 → VMess 或直连。
界面用 React，逻辑全在 Rust，全部跑在 Tauri 的 tokio 运行时里，**没有 sidecar、没有 TUN、没有系统服务**。

明确不做：Electron、Go 子进程、wintun、UDP 转发、VMess 的 ws/grpc/TLS（当前只做 AEAD + TCP）。

## 2. 工具链与命令

| 场景 | 命令 |
| --- | --- |
| 安装依赖 | `bun install` |
| 前端开发（浏览器，自动演示数据） | `bun run dev` |
| 桌面端开发（推荐） | `bun run tauri:dev` |
| 前端类型检查 + 构建 | `bun run build` |
| 打包 NSIS | `bun run tauri:build` |
| 后端格式化 | `cd src-tauri && cargo fmt` |
| 静态检查（必须 0 警告） | `cargo clippy --lib --tests` |
| 单元测试 | `cargo test --lib` |
| VMess 真实互操作测试 | `cargo test --test vmess_e2e -- --ignored --test-threads=1` |

版本基线：Rust 1.90+、Tauri 2.12、React 19、Vite 5、Tailwind 3.4、bun 1.4。

## 3. 目录地图

```
src/                         前端
├── App.tsx                  页面切换（hash 路由）、事件订阅、快捷键
├── pages/                   四个页面：Home / Nodes / Rules / Settings
├── components/              TitleBar、NavRail、Sheet、ConfirmDialog、RuleEditor…
├── store/useAppStore.ts     连接、流量、节点、设置、toast
├── store/useRuleStore.ts    自定义规则、订阅列表、规则测试
├── lib/tauri.ts             invoke / listen 封装、错误归一化、窗口操作
├── lib/types.ts             与后端 serde 一一对应的类型
├── lib/mock.ts              浏览器预览用的演示数据（含已连接状态）
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
├── vmess/                   AEAD、头部、分块、TCP 传输
├── sysproxy/                系统代理快照/写入/还原、代理环境变量、开机自启
└── resources/               内置 CN 网段（cn_ipv4.txt / cn_ipv6.txt）
```

## 4. 运行架构

### 启动时序（`lib.rs`）

1. `AppState::new(app_config_dir)`：加载 `config.json` → 若 `proxy_dirty` 为真则立即还原系统代理 → 编译规则集 → 创建 `RouterHandle`。
2. `app.manage(Arc<AppState>)`。
3. `setup_tray`：托盘菜单 id 为 `show` / `toggle` / `quit`。
4. `spawn_traffic`：每秒 `stats.sample()` 并 `emit("nano:traffic")`。
5. `spawn_remote_scheduler`：每 60 秒检查一次到期订阅，到期就更新并 `emit` 事件。
6. `RunEvent::ExitRequested` 时同步调用 `state.restore_system_proxy()`。

### 连接生命周期（`proxy/inbound`）

```
TcpStream ──peek 首字节──> 0x05 走 SOCKS5，否则走 HTTP
   → Accepted { Target, preface, kind }
   → RouterHandle.get().decide(target)   // 可能触发 DNS
   → Policy::Proxy ? VmessOutbound : DirectOutbound
   → reply_success()
   → 先写 preface（HTTP 明文请求 / CONNECT 多读的字节）
   → 双向 copy，边转发边累加 TrafficStats
```

- HTTP 只支持 `CONNECT host:port` 与绝对 URI（`GET http://host/path`），每连接一个请求，不复用。
- SOCKS5 只接受 no-auth + CONNECT；UDP ASSOCIATE 回复 `0x07`。
- 拒绝策略返回 HTTP 403 / SOCKS5 `0x02`；上游失败返回 502 / `0x05`。

### 规则热替换

`AppState.router` 是 `Arc<RouterHandle>`，内部 `RwLock<Arc<Router>>`；入站持有句柄，每个新连接重新取。
修改自定义规则、订阅、默认策略、DNS 时调用 `AppState::reload_rules()` 重建 `Router` 并整体替换，
**已建立的连接不受影响**。路由决策有 120 秒 TTL、上限 1 万条的域名缓存，随 Router 一起丢弃。

### 事件

| 事件 | 负载 | 触发 |
| --- | --- | --- |
| `nano:traffic` | `TrafficPayload` | 每秒 |
| `nano:proxy` | `ProxyStatus` | 启停、设置变更、切换节点 |
| `nano:rules` | `RulesSummary` | 规则重编译 |
| `nano:remote` | `RemoteOutcome` | 单个订阅更新完成 |

前端在 `App.tsx` 统一订阅；组件只从 store 读，不直接 listen。

## 5. 数据与 IPC 契约

### 配置文件

- 位置：`app.path().app_config_dir()`，即 `%APPDATA%\com.nanoshadowrocket.desktop\config.json`。
- 规则缓存：同目录 `rules/<订阅 id>.conf`。
- 写入方式：临时文件 + 原子 rename（`config/store.rs`）；解析失败会把原文件备份成 `config.json.bak` 并回退默认值。
- 字段全部 `camelCase`（`#[serde(rename_all = "camelCase", default)]`），前端 `lib/types.ts` 必须同步。

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

## 7. VMess 实现范围（`src-tauri/src/vmess`）

- 仅 **AEAD**（`alterId = 0`）、`net = tcp`、`security = aes-128-gcm | chacha20-poly1305`。
- `aead.rs`：KDF、cmdKey、authID、两种 AEAD 封装。
- `header.rs`：请求头明文布局、AEAD 分片、响应头解析；`chunk.rs`：请求/响应分块（≤16KB + tag）。
- `transport.rs`：TcpStream → 握手 → `VmessStream`（实现 `AsyncRead + AsyncWrite`）。
- **响应头是惰性的**：`connect()` 只发请求头，不等待响应头；第一次读数据时才校验响应认证。
  这是与官方 v2ray-core 互通的关键，改动前先跑 `tests/vmess_e2e.rs`。
- 新增传输（ws / tls / grpc）时的正确做法：在 `outbound.rs` 增加新的 `Outbound` 实现或
  给 `VmessOutbound` 注入 transport 工厂，**不要**把传输细节写进 `chunk.rs` / `header.rs`。

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
- 开机自启写 `HKCU\...\CurrentVersion\Run` 的 `NanoShadowrocket` 值。
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
- **Portal**：`Sheet` / `ConfirmDialog` 必须 `createPortal(..., document.body)`。页面根节点有
  `animate-fade-up`（带 transform），会让子级 `position: fixed` 相对页面而不是视口定位。
- **路由**：无 react-router，用 `location.hash`（`#/home`、`#/nodes`、`#/rules`、`#/settings`）并监听 `hashchange`。
- **样式**：颜色 token 在 `tailwind.config.js`（`base/surface/elevated/ink/accent/ok/danger/warn/info`）。
  自定义 rgba 颜色不要再叠加 `/opacity` 修饰符（如 `bg-accent-soft/50` 会得到意料之外的亮度）；
  需要透明度时直接用任意值（`bg-[rgba(...)]` / `bg-white/[0.04]`）。
- **交互**：所有可点击元素必须是 `<button>`；开关用 `Switch`（role=switch）；表格/列表行高稳定，避免 hover 时布局跳动。
- **响应式**：断点用 `max-[860px]:`（窗口最窄 780px），主卡在窄窗口改为纵向堆叠。

## 10. 测试

- 单测跟模块放在一起（`#[cfg(test)] mod tests`），覆盖：规则解析/匹配、VMess AEAD 与分块、
  `vmess://` 解析、配置往返、错误码、SOCKS5 握手、HTTP 头解析、统计速率。
- `tests/vmess_e2e.rs` 需要真实 v2ray-core（`127.0.0.1:10086`）与 `python -m http.server 18080`，
  默认 `#[ignore]`。注意 `#[ignore = "..."]` 的值必须是**字面量**，不能用常量。
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
- 发版前同步版本号：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、
  `TitleBar.tsx` 里的 `v0.1.0`。
- 应用标识符 `com.nanoshadowrocket.desktop` 一经发布不要改，否则配置目录与升级链会断。

### 截图更新

```bash
bun run dev
# 另开终端，用 Playwright/无头 Chromium 按 900x760（规则页 900x880）截图
# 覆盖 docs/screenshots/{home,nodes,rules,settings,node-form,rule-test}.png
```

截图依赖 `lib/mock.ts` 的演示数据；修改演示节点或订阅后记得同步重截。

## 12. 常见任务

**新增一种规则类型**

1. `rules/parser.rs`：加 `RuleKind` 分支、解析与校验、`label()`/`value()`。
2. `rules/matcher.rs`：建索引、加入 `decide_domain` / `decide_ip` 的候选比较与 `needs_resolve` 判断。
3. `RuleEditor.tsx` 的高亮规则（如需要）。
4. 补解析与匹配单测。

**新增一个命令**

1. 在 `commands/` 对应模块写 `#[tauri::command]`，参数结构体用 `#[serde(rename_all = "camelCase")]`。
2. `lib.rs` 的 `generate_handler!` 注册。
3. `src/lib/types.ts` + `src/lib/tauri.ts` 封装，页面通过 store 调用。
4. 返回 `Result<T, AppError>`；Tauri 会把它序列化成 `{ code, message }`。

**新增页面**

`components/NavRail.tsx` 的 `ITEMS` → `App.tsx` 的 `PAGES` 与渲染分支 → `pages/XxxPage.tsx`。

**换/加出站类型**

实现 `proxy::Outbound`（`async fn connect(&self, &Target) -> Result<BoxedStream>`），
在 `AppState::start` 里按配置选择，路由与 UI 不需要改。

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

## 14. 代码风格

- Rust：`rustfmt` 默认风格；`thiserror` 定义错误；`log::{info,warn,debug}`，不要 `println!`；
  公共 API 写一行文档注释，模块头部写职责与边界。
- TypeScript：组件 PascalCase、工具函数 camelCase；避免 `any`；格式化/纯函数放 `lib/`；
  组件只做渲染，业务状态进 store。
- 注释只解释「为什么」，不复述代码。
