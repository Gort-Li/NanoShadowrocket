//! 内置 v2ray-core sidecar 的生命周期管理。
//!
//! 每个 VMess 节点对应一个本地 SOCKS 入站。NanoShadowrocket 只把它当作
//! 上游出站使用：TCP 走 SOCKS CONNECT，UDP 走 SOCKS UDP ASSOCIATE。
//! 这样 legacy `alterId`、VMess UDP 以及未来的 ws/tls 都能复用成熟实现。

mod config;
#[cfg(windows)]
mod job;

use std::fs::File;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::config::Transport;
use crate::error::AppError;
use crate::vmess::VmessConfig;

const READY_TIMEOUT: Duration = Duration::from_secs(8);
const READY_INTERVAL: Duration = Duration::from_millis(60);
const LOG_TAIL: usize = 2048;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 一个正在运行的 v2ray-core 子进程。
pub struct V2raySidecar {
    child: Child,
    socks: SocketAddr,
    config_path: PathBuf,
    #[cfg(windows)]
    _job: Option<job::JobHandle>,
}

impl V2raySidecar {
    /// 启动 sidecar 并等待本地 SOCKS 端口可连接。
    pub async fn start(
        config: &VmessConfig,
        transport: Transport,
        config_dir: &Path,
    ) -> Result<Self, AppError> {
        if !transport.supported() {
            return Err(AppError::Unsupported(format!(
                "v2ray 内核暂未接入 {} 传输",
                transport.as_str()
            )));
        }

        let binary = binary_path()?;
        let port = pick_port()?;
        let socks = SocketAddr::from(([127, 0, 0, 1], port));
        let config_path = config_dir.join("v2ray-sidecar.json");
        let log_path = config_dir.join("v2ray-sidecar.log");

        std::fs::create_dir_all(config_dir)
            .map_err(|e| AppError::Sidecar(format!("创建配置目录失败：{e}")))?;

        let json = config::build(config, port);
        let bytes = serde_json::to_vec_pretty(&json)
            .map_err(|e| AppError::Sidecar(format!("生成 v2ray 配置失败：{e}")))?;
        if let Err(err) = std::fs::write(&config_path, bytes) {
            // 写失败也可能留下半截文件，不能让它带着 UUID 留在磁盘上。
            let _ = std::fs::remove_file(&config_path);
            return Err(AppError::Sidecar(format!("写入 v2ray 配置失败：{err}")));
        }

        let log = match File::create(&log_path) {
            Ok(log) => log,
            Err(err) => {
                let _ = std::fs::remove_file(&config_path);
                return Err(AppError::Sidecar(format!("创建 v2ray 日志失败：{err}")));
            }
        };
        let log_err = match log.try_clone() {
            Ok(log) => log,
            Err(err) => {
                let _ = std::fs::remove_file(&config_path);
                return Err(AppError::Sidecar(format!("复制 v2ray 日志句柄失败：{err}")));
            }
        };

        let mut command = Command::new(&binary);
        command
            .arg("run")
            .arg("-c")
            .arg(&config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let _ = std::fs::remove_file(&config_path);
                return Err(AppError::Sidecar(format!("启动 v2ray 内核失败：{err}")));
            }
        };

        #[cfg(windows)]
        let job = job::JobHandle::assign(&child);

        if let Err(err) = wait_ready(&mut child, socks, &log_path).await {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&config_path);
            return Err(err);
        }

        log::info!("v2ray 内核已启动，本地 SOCKS 监听 {socks}");
        Ok(Self {
            child,
            socks,
            config_path,
            #[cfg(windows)]
            _job: job,
        })
    }

    pub fn socks_addr(&self) -> SocketAddr {
        self.socks
    }
}

impl Drop for V2raySidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.config_path);
        #[cfg(windows)]
        {
            self._job.take();
        }
    }
}

fn binary_path() -> Result<PathBuf, AppError> {
    let exe =
        std::env::current_exe().map_err(|e| AppError::Sidecar(format!("无法定位程序目录：{e}")))?;
    let dir = exe
        .parent()
        .ok_or_else(|| AppError::Sidecar("无法定位程序目录".into()))?;
    let bundled = dir.join("v2ray.exe");
    if bundled.is_file() {
        return Ok(bundled);
    }

    // 开发态（tauri dev / cargo test）可能还没把 externalBin 复制到 target/debug，
    // 回退到仓库里的构建产物；release 只认安装包同目录的 v2ray.exe。
    #[cfg(debug_assertions)]
    {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join("v2ray-x86_64-pc-windows-msvc.exe");
        if dev.is_file() {
            return Ok(dev);
        }
    }

    Err(AppError::Sidecar(
        "未找到内置 v2ray 内核，请重新安装，或在源码目录运行 bun run prepare:sidecar".into(),
    ))
}

fn pick_port() -> Result<u16, AppError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| AppError::Sidecar(format!("分配本地端口失败：{e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::Sidecar(format!("读取本地端口失败：{e}")))?
        .port();
    drop(listener);
    Ok(port)
}

async fn wait_ready(child: &mut Child, socks: SocketAddr, log_path: &Path) -> Result<(), AppError> {
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(AppError::Sidecar(format!(
                "v2ray 内核提前退出（{status}）：{}",
                log_tail(log_path)
            )));
        }

        if tokio::time::timeout(
            Duration::from_millis(200),
            tokio::net::TcpStream::connect(socks),
        )
        .await
        .is_ok_and(|result| result.is_ok())
        {
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::Sidecar(format!(
                "v2ray 内核启动超时：{}",
                log_tail(log_path)
            )));
        }
        tokio::time::sleep(READY_INTERVAL).await;
    }
}

fn log_tail(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return "（没有日志）".into();
    };
    let tail: String = text
        .chars()
        .rev()
        .take(LOG_TAIL)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if tail.trim().is_empty() {
        "（日志为空）".into()
    } else {
        tail.trim().to_string()
    }
}
