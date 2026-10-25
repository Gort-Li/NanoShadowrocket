//! 混合入站：单 TCP 端口同时服务 HTTP 代理与 SOCKS5 客户端。
//!
//! 首字节 `0x05` 判为 SOCKS5，其余按 HTTP 解析。两种协议最终都归一成
//! [`Target`]，之后统一走「路由决策 → 出站 → 双向转发」。
//!
//! HTTP 只支持 CONNECT 与绝对 URI 请求，且每个连接只处理一个请求（不复用），
//! 这是代理场景下最常见也最不容易出错的形态。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::outbound::{BoxedStream, Outbound};
use super::router::RouterHandle;
use super::stats::{ActiveConnections, ConnectionProtocol, TrafficStats};
use super::target::Target;
use crate::error::{bind_error, AppError};
use crate::rules::Policy;

mod http;
mod socks5;
mod udp;

/// 入站依赖。启动后不可变，规则热更新通过替换 [`Router`] 实现。
pub struct InboundDeps {
    pub router: Arc<RouterHandle>,
    pub proxy: Arc<dyn Outbound>,
    pub direct: Arc<dyn Outbound>,
    pub proxy_udp: Arc<dyn super::udp::UdpOutbound>,
    pub direct_udp: Arc<dyn super::udp::UdpOutbound>,
    pub udp_enabled: bool,
    pub stats: Arc<TrafficStats>,
    pub active: Arc<ActiveConnections>,
}

enum ClientKind {
    HttpConnect,
    HttpPlain,
    Socks5,
}

struct Accepted {
    target: Target,
    /// 需要在转发前写给上游的字节（HTTP 改写后的请求头与可能多读的 body）。
    preface: Vec<u8>,
    kind: ClientKind,
}

pub struct InboundHandle {
    local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl InboundHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = self.task.await;
    }
}

/// 绑定端口并启动 accept 循环。
pub async fn start(bind: SocketAddr, deps: InboundDeps) -> Result<InboundHandle, AppError> {
    let listener = TcpListener::bind(bind)
        .await
        .map_err(|err| bind_error(bind.port(), &err))?;
    let local_addr = listener.local_addr()?;
    log::info!("混合入站已监听 {local_addr}");

    let deps = Arc::new(deps);
    let (shutdown, mut stop) = watch::channel(false);
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, peer)) => {
                            let deps = deps.clone();
                            tokio::spawn(async move { handle(stream, peer, deps).await });
                        }
                        Err(err) => {
                            log::warn!("接受连接失败：{err}");
                            tokio::time::sleep(Duration::from_millis(80)).await;
                        }
                    }
                }
            }
        }
        log::info!("混合入站已停止");
    });

    Ok(InboundHandle {
        local_addr,
        shutdown,
        task,
    })
}

async fn handle(mut client: TcpStream, peer: SocketAddr, deps: Arc<InboundDeps>) {
    let _active = deps.stats.enter();
    if let Err(err) = serve(&mut client, peer, &deps).await {
        log::debug!("来自 {peer} 的连接结束：{err}");
    }
}

async fn serve(client: &mut TcpStream, peer: SocketAddr, deps: &InboundDeps) -> Result<(), String> {
    let mut first = [0u8; 1];
    let n = client.peek(&mut first).await.map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(());
    }

    let accepted = if first[0] == 0x05 {
        let request = match socks5::handshake(client).await {
            Ok(request) => request,
            Err(err) => {
                if let Some(code) = err.reply {
                    let _ = socks5::reply(client, code).await;
                }
                log::debug!("SOCKS5 握手失败：{err}");
                return Err(err.to_string());
            }
        };
        let target = match request {
            socks5::Request::Connect(target) => target,
            socks5::Request::UdpAssociate { .. } => {
                if !deps.udp_enabled {
                    let _ = socks5::reply(client, 0x07).await;
                    return Ok(());
                }
                return udp::serve(client, peer, deps).await;
            }
        };
        Accepted {
            target,
            preface: Vec::new(),
            kind: ClientKind::Socks5,
        }
    } else {
        let (head, extra) = http::read_request(client)
            .await
            .map_err(|e| e.to_string())?;
        let mut preface = head.forward;
        if !extra.is_empty() {
            preface.extend_from_slice(&extra);
        }
        Accepted {
            target: head.target,
            preface,
            kind: if head.is_connect {
                ClientKind::HttpConnect
            } else {
                ClientKind::HttpPlain
            },
        }
    };

    let decision = deps.router.get().decide(&accepted.target).await;

    if decision.is_reject() {
        reply_error(client, accepted.kind, ReplyKind::Rejected).await;
        return Ok(());
    }

    // guard 一直活到 serve 结束；连接失败 / 客户端断开都会自动移除。
    let _connection = deps.active.begin(
        accepted.target.host(),
        accepted.target.port(),
        decision.policy,
        decision.rule.clone(),
        ConnectionProtocol::Tcp,
    );

    let outbound = match decision.policy {
        Policy::Proxy => &deps.proxy,
        Policy::Direct | Policy::Reject => &deps.direct,
    };
    let server = match outbound.connect(&accepted.target).await {
        Ok(stream) => stream,
        Err(err) => {
            log::debug!(
                "连接 {} 失败（{}）：{err}",
                accepted.target.authority(),
                outbound.kind()
            );
            reply_error(client, accepted.kind, ReplyKind::Upstream).await;
            return Ok(());
        }
    };

    reply_success(client, accepted.kind)
        .await
        .map_err(|e| e.to_string())?;
    relay(client, server, &accepted.preface, &deps.stats).await;
    Ok(())
}

enum ReplyKind {
    Rejected,
    Upstream,
}

async fn reply_success(client: &mut TcpStream, kind: ClientKind) -> std::io::Result<()> {
    match kind {
        ClientKind::HttpConnect => http::reply_connect_ok(client).await,
        ClientKind::HttpPlain => Ok(()),
        ClientKind::Socks5 => socks5::reply(client, 0x00).await,
    }
}

async fn reply_error(client: &mut TcpStream, kind: ClientKind, reason: ReplyKind) {
    let result = match kind {
        ClientKind::HttpConnect | ClientKind::HttpPlain => {
            let (status, body) = match reason {
                ReplyKind::Rejected => (403, "已按规则拒绝该请求"),
                ReplyKind::Upstream => (502, "上游节点连接失败"),
            };
            http::reply_error(client, status, body).await
        }
        ClientKind::Socks5 => {
            let code = match reason {
                ReplyKind::Rejected => 0x02, // connection not allowed by ruleset
                ReplyKind::Upstream => 0x05, // connection refused
            };
            socks5::reply(client, code).await
        }
    };
    if let Err(err) = result {
        log::debug!("回复错误响应失败：{err}");
    }
}

async fn relay(
    client: &mut TcpStream,
    mut server: BoxedStream,
    preface: &[u8],
    stats: &TrafficStats,
) {
    if !preface.is_empty() {
        if let Err(err) = server.write_all(preface).await {
            log::debug!("写入首包失败：{err}");
            return;
        }
        if let Err(err) = server.flush().await {
            log::debug!("刷新首包失败：{err}");
            return;
        }
        stats.add_up(preface.len() as u64);
    }

    let (mut client_read, mut client_write) = tokio::io::split(client);
    let (mut server_read, mut server_write) = tokio::io::split(server);

    let upload = async {
        let result = copy_count(&mut client_read, &mut server_write, stats, true).await;
        let _ = server_write.shutdown().await;
        result
    };
    let download = async {
        let result = copy_count(&mut server_read, &mut client_write, stats, false).await;
        let _ = client_write.shutdown().await;
        result
    };

    let (up, down) = tokio::join!(upload, download);
    if let Err(err) = up {
        log::debug!("上行转发中断：{err}");
    }
    if let Err(err) = down {
        log::debug!("下行转发中断：{err}");
    }
}

async fn copy_count<R, W>(
    reader: &mut R,
    writer: &mut W,
    stats: &TrafficStats,
    upload: bool,
) -> std::io::Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 16 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await?;
        writer.flush().await?;
        total += n as u64;
        if upload {
            stats.add_up(n as u64);
        } else {
            stats.add_down(n as u64);
        }
    }
    Ok(total)
}
