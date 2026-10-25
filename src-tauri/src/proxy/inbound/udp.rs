//! SOCKS5 UDP ASSOCIATE 的入站实现。
//!
//! 每个关联有独立的 UDP socket；TCP 控制连接关闭时关联结束。
//! 客户端地址在第一个数据报到达时固定，防止被当作开放 UDP 中继。

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{mpsc, watch};

use super::socks5;
use super::InboundDeps;
use crate::proxy::socks5_client::{decode_udp, encode_udp};
use crate::proxy::stats::{ConnectionGuard, ConnectionProtocol};
use crate::proxy::target::Target;
use crate::proxy::udp::{UdpFlow, UdpOutbound};
use crate::rules::Policy;

const MAX_UDP_PACKET: usize = 65535;

type UdpResponse = (Policy, Result<(Target, Vec<u8>), String>);

pub async fn serve(
    control: &mut TcpStream,
    peer: SocketAddr,
    deps: &InboundDeps,
) -> Result<(), String> {
    let local = control.local_addr().map_err(|e| e.to_string())?;
    let bind_ip = match local.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        ip => ip,
    };
    let socket = UdpSocket::bind(SocketAddr::new(bind_ip, 0))
        .await
        .map_err(|e| format!("绑定 UDP 端口失败：{e}"))?;
    let bound = socket.local_addr().map_err(|e| e.to_string())?;
    socks5::reply_udp(control, bound)
        .await
        .map_err(|e| format!("回复 UDP ASSOCIATE 失败：{e}"))?;

    let socket = Arc::new(socket);
    let (tx, mut rx) = mpsc::channel::<UdpResponse>(256);
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut flows: HashMap<Policy, Arc<dyn UdpFlow>> = HashMap::new();
    let mut client_addr: Option<SocketAddr> = None;
    let mut active_entries: HashMap<String, ConnectionGuard<'_>> = HashMap::new();
    let mut buf = vec![0u8; MAX_UDP_PACKET];
    let mut control_buf = [0u8; 1];

    loop {
        tokio::select! {
            result = control.read(&mut control_buf) => {
                match result {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
            received = socket.recv_from(&mut buf) => {
                let (n, source) = match received {
                    Ok(value) => value,
                    Err(err) => {
                        log::debug!("UDP 接收失败：{err}");
                        break;
                    }
                };
                if source.ip() != peer.ip() {
                    continue;
                }
                match client_addr {
                    Some(pinned) if pinned != source => continue,
                    None => client_addr = Some(source),
                    _ => {}
                }

                let packet = buf[..n].to_vec();
                let (target, data) = match decode_udp(&packet) {
                    Ok(value) => (value.0, value.1.to_vec()),
                    Err(err) => {
                        log::debug!("忽略非法 UDP 包：{err}");
                        continue;
                    }
                };
                let decision = deps.router.get().decide(&target).await;
                if decision.is_reject() {
                    continue;
                }

                let key = target.authority();
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    active_entries.entry(key)
                {
                    let guard = deps.active.begin(
                        target.host(),
                        target.port(),
                        decision.policy,
                        decision.rule.clone(),
                        ConnectionProtocol::Udp,
                    );
                    entry.insert(guard);
                }

                let target = if decision.policy == Policy::Direct {
                    match &target {
                        Target::Domain(host, port) => {
                            let ips = deps.router.get().resolve(host).await.unwrap_or_default();
                            let Some(ip) = ips.first().copied() else {
                                continue;
                            };
                            Target::Ip(ip, *port)
                        }
                        _ => target,
                    }
                } else {
                    target
                };

                let Some(flow) = ensure_flow(
                    decision.policy,
                    deps,
                    &mut flows,
                    tx.clone(),
                    stop_rx.clone(),
                )
                .await
                else {
                    continue;
                };
                match flow.send(&target, &data).await {
                    Ok(()) => deps.stats.add_up(data.len() as u64),
                    Err(err) => log::debug!("UDP 上行发送失败：{err}"),
                }
            }
            response = rx.recv() => {
                let Some((policy, result)) = response else {
                    break;
                };
                let (source, data) = match result {
                    Ok(value) => value,
                    Err(err) => {
                        // 丢弃坏掉的 flow，下一个数据报会重新建立关联。
                        log::debug!("UDP 出站已结束（{}）：{err}", policy.as_str());
                        flows.remove(&policy);
                        continue;
                    }
                };
                let Some(client) = client_addr else {
                    continue;
                };
                let packet = match encode_udp(&source, &data) {
                    Ok(value) => value,
                    Err(err) => {
                        log::debug!("封装 UDP 响应失败：{err}");
                        continue;
                    }
                };
                if socket.send_to(&packet, client).await.is_ok() {
                    deps.stats.add_down(data.len() as u64);
                }
            }
        }
    }

    let _ = stop_tx.send(true);
    Ok(())
}

async fn ensure_flow(
    policy: Policy,
    deps: &InboundDeps,
    flows: &mut HashMap<Policy, Arc<dyn UdpFlow>>,
    tx: mpsc::Sender<UdpResponse>,
    stop: watch::Receiver<bool>,
) -> Option<Arc<dyn UdpFlow>> {
    if let Some(flow) = flows.get(&policy) {
        return Some(flow.clone());
    }

    let outbound: &Arc<dyn UdpOutbound> = match policy {
        Policy::Proxy => &deps.proxy_udp,
        Policy::Direct => &deps.direct_udp,
        Policy::Reject => return None,
    };
    let flow: Arc<dyn UdpFlow> = match outbound.open().await {
        Ok(flow) => Arc::from(flow),
        Err(err) => {
            log::debug!("创建 UDP 出站失败（{}）：{err}", outbound.kind());
            return None;
        }
    };
    flows.insert(policy, flow.clone());

    let flow_for_task = flow.clone();
    tokio::spawn(async move {
        let mut stop = stop;
        loop {
            tokio::select! {
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        break;
                    }
                }
                result = flow_for_task.recv() => {
                    match result {
                        Ok(value) => {
                            if tx.send((policy, Ok(value))).await.is_err() {
                                break;
                            }
                        }
                        Err(err) => {
                            let _ = tx.send((policy, Err(err.to_string()))).await;
                            break;
                        }
                    }
                }
            }
        }
    });

    Some(flow)
}
