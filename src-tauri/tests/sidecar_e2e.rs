//! 内置 v2ray-core sidecar 的真实互操作测试（默认跳过）。
//!
//! 会自行拉起一个本地 v2ray-core 服务端（alterId = 64），验证：
//! 1. legacy VMess 请求头可以完成 TCP 转发；
//! 2. SOCKS5 UDP ASSOCIATE 可以完成 UDP 往返。
//!
//! 运行前先准备二进制：`bun run prepare:sidecar`，然后：
//! `cargo test --test sidecar_e2e -- --ignored --test-threads=1`

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use app_lib::config::{NodeEntry, Transport};
use app_lib::proxy::{
    socks5_client, ConnectionProtocol, Outbound, SidecarOutbound, SidecarUdpOutbound, Target,
    UdpOutbound,
};
use app_lib::rules::Policy;
use app_lib::sidecar::V2raySidecar;
use app_lib::state::AppState;
use app_lib::vmess::{CipherMethod, VmessConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

const UUID: &str = "b831381d-6324-4d53-ad4f-8cda48b30811";
const STEP_TIMEOUT: Duration = Duration::from_secs(8);

struct Server {
    child: Child,
    config_path: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.config_path);
    }
}

fn binary() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join("v2ray-x86_64-pc-windows-msvc.exe")
}

async fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

async fn start_server(alter_id: u16) -> (Server, u16) {
    let binary = binary();
    assert!(
        binary.is_file(),
        "缺少 {}，请先运行 bun run prepare:sidecar",
        binary.display()
    );

    let port = free_port().await;
    let config = format!(
        r#"{{
  "log": {{ "loglevel": "warning" }},
  "inbounds": [
    {{
      "listen": "127.0.0.1",
      "port": {port},
      "protocol": "vmess",
      "settings": {{
        "clients": [
          {{ "id": "{UUID}", "alterId": {alter_id}, "security": "auto" }}
        ]
      }}
    }}
  ],
  "outbounds": [
    {{ "protocol": "freedom", "settings": {{}} }}
  ]
}}"#
    );
    let config_path = std::env::temp_dir().join(format!(
        "nano-v2ray-server-{}-{port}.json",
        std::process::id()
    ));
    std::fs::write(&config_path, config).unwrap();

    let mut command = Command::new(&binary);
    command
        .arg("run")
        .arg("-c")
        .arg(&config_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("V2RAY_VMESS_AEAD_FORCED", "false");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().expect("启动 v2ray 服务端失败");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let _ = std::fs::remove_file(&config_path);
            panic!("v2ray 服务端提前退出：{status}");
        }
        if tokio::time::timeout(
            Duration::from_millis(200),
            TcpStream::connect(("127.0.0.1", port)),
        )
        .await
        .is_ok_and(|result| result.is_ok())
        {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&config_path);
            panic!("v2ray 服务端启动超时");
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
    }

    (Server { child, config_path }, port)
}

fn client_config(port: u16, alter_id: u16, security: CipherMethod) -> VmessConfig {
    VmessConfig {
        host: "127.0.0.1".into(),
        port,
        uuid: UUID.into(),
        alter_id,
        security,
    }
}

async fn tcp_echo_roundtrip(socks: SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let echo = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4];
        socket.read_exact(&mut buf).await.unwrap();
        socket.write_all(&buf).await.unwrap();
    });
    let outbound = SidecarOutbound::new(socks);
    let target = Target::Ip("127.0.0.1".parse().unwrap(), port);
    let mut stream = tokio::time::timeout(STEP_TIMEOUT, outbound.connect(&target))
        .await
        .expect("TCP 握手超时")
        .expect("TCP 握手失败");
    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    tokio::time::timeout(STEP_TIMEOUT, stream.read_exact(&mut echoed))
        .await
        .expect("TCP 读取超时")
        .unwrap();
    assert_eq!(&echoed, b"ping");
    echo.await.unwrap();
}

#[tokio::test]
#[ignore = "需要 src-tauri/binaries/v2ray-x86_64-pc-windows-msvc.exe（bun run prepare:sidecar）"]
async fn aead_alter_id_zero_tcp() {
    let (_server, server_port) = start_server(0).await;
    let config_dir = std::env::temp_dir().join(format!("nano-sidecar-aead-{}", std::process::id()));
    std::fs::create_dir_all(&config_dir).unwrap();
    let sidecar = V2raySidecar::start(
        &client_config(server_port, 0, CipherMethod::Aes128Gcm),
        Transport::Tcp,
        &config_dir,
    )
    .await
    .expect("启动 sidecar 失败");
    tcp_echo_roundtrip(sidecar.socks_addr()).await;
    drop(sidecar);
    let _ = std::fs::remove_dir_all(&config_dir);
}

#[tokio::test]
#[ignore = "需要 src-tauri/binaries/v2ray-x86_64-pc-windows-msvc.exe（bun run prepare:sidecar）"]
async fn legacy_alter_id_tcp_and_udp() {
    let (_server, server_port) = start_server(64).await;
    let config_dir =
        std::env::temp_dir().join(format!("nano-sidecar-client-{}", std::process::id()));
    std::fs::create_dir_all(&config_dir).unwrap();
    let sidecar = V2raySidecar::start(
        &client_config(server_port, 64, CipherMethod::Legacy),
        Transport::Tcp,
        &config_dir,
    )
    .await
    .expect("启动 sidecar 失败");
    let socks = sidecar.socks_addr();

    // TCP：本地 echo -> VMess(alterId=64) -> 服务端 freedom -> echo。
    let tcp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_port = tcp_listener.local_addr().unwrap().port();
    let tcp_echo = tokio::spawn(async move {
        let (mut socket, _) = tcp_listener.accept().await.unwrap();
        let mut buf = [0u8; 4];
        socket.read_exact(&mut buf).await.unwrap();
        socket.write_all(&buf).await.unwrap();
    });
    let outbound = SidecarOutbound::new(socks);
    let target = Target::Ip("127.0.0.1".parse().unwrap(), tcp_port);
    let mut stream = tokio::time::timeout(STEP_TIMEOUT, outbound.connect(&target))
        .await
        .expect("TCP 握手超时")
        .expect("TCP 握手失败");
    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    tokio::time::timeout(STEP_TIMEOUT, stream.read_exact(&mut echoed))
        .await
        .expect("TCP 读取超时")
        .unwrap();
    assert_eq!(&echoed, b"ping");
    tcp_echo.await.unwrap();

    // UDP：本地 echo -> SOCKS UDP -> VMess UDP -> 服务端 freedom。
    let udp_echo = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let udp_port = udp_echo.local_addr().unwrap().port();
    let udp_task = tokio::spawn(async move {
        let mut buf = [0u8; 32];
        let (n, source) = udp_echo.recv_from(&mut buf).await.unwrap();
        udp_echo.send_to(&buf[..n], source).await.unwrap();
    });
    let udp = SidecarUdpOutbound::new(socks);
    let flow = tokio::time::timeout(STEP_TIMEOUT, udp.open())
        .await
        .expect("UDP 关联超时")
        .expect("UDP 关联失败");
    flow.send(&Target::Ip("127.0.0.1".parse().unwrap(), udp_port), b"pong")
        .await
        .unwrap();
    let (source, data) = tokio::time::timeout(STEP_TIMEOUT, flow.recv())
        .await
        .expect("UDP 读取超时")
        .unwrap();
    assert_eq!(source.port(), udp_port);
    assert_eq!(data, b"pong");
    udp_task.await.unwrap();

    drop(flow);
    drop(sidecar);
    let _ = std::fs::remove_dir_all(&config_dir);
}

#[tokio::test]
#[ignore = "需要 src-tauri/binaries/v2ray-x86_64-pc-windows-msvc.exe（bun run prepare:sidecar）"]
async fn app_state_routes_legacy_tcp_and_udp() {
    let (_server, server_port) = start_server(64).await;
    let config_dir = std::env::temp_dir().join(format!("nano-app-sidecar-{}", std::process::id()));
    std::fs::create_dir_all(&config_dir).unwrap();
    let state = AppState::new(&config_dir).unwrap();
    state
        .update_config(|cfg| {
            cfg.listen_port = 0;
            cfg.allow_lan = false;
            cfg.system_proxy = false;
            cfg.proxy_env = false;
            cfg.default_policy = Policy::Proxy;
            cfg.custom_rules = "IP-CIDR,127.0.0.1/32,PROXY,no-resolve\n".into();
            cfg.nodes = vec![NodeEntry {
                id: "legacy-test".into(),
                name: "legacy-test".into(),
                transport: Transport::Tcp,
                config: VmessConfig {
                    host: "127.0.0.1".into(),
                    port: server_port,
                    uuid: UUID.into(),
                    alter_id: 64,
                    security: CipherMethod::Legacy,
                },
                udp: true,
                latency_ms: None,
            }];
            cfg.selected_node = Some("legacy-test".into());
        })
        .unwrap();

    let status = state.start().await.expect("启动完整代理失败");
    let inbound = SocketAddr::from(([127, 0, 0, 1], status.local_port));

    let tcp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_port = tcp_listener.local_addr().unwrap().port();
    let tcp_echo = tokio::spawn(async move {
        let (mut socket, _) = tcp_listener.accept().await.unwrap();
        let mut buf = [0u8; 4];
        socket.read_exact(&mut buf).await.unwrap();
        socket.write_all(&buf).await.unwrap();
    });
    let target = Target::Ip("127.0.0.1".parse().unwrap(), tcp_port);
    let mut stream = tokio::time::timeout(STEP_TIMEOUT, socks5_client::connect(inbound, &target))
        .await
        .expect("入站 TCP 握手超时")
        .expect("入站 TCP 握手失败");
    let active = state.active.snapshot();
    assert!(
        active.iter().any(|item| {
            item.host == "127.0.0.1"
                && item.port == tcp_port
                && item.protocol == ConnectionProtocol::Tcp
        }),
        "TCP 连接应出现在活动连接列表：{active:?}"
    );
    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    tokio::time::timeout(STEP_TIMEOUT, stream.read_exact(&mut echoed))
        .await
        .expect("入站 TCP 读取超时")
        .unwrap();
    assert_eq!(&echoed, b"ping");
    tcp_echo.await.unwrap();

    let udp_echo = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let udp_port = udp_echo.local_addr().unwrap().port();
    let udp_task = tokio::spawn(async move {
        let mut buf = [0u8; 32];
        let (n, source) = udp_echo.recv_from(&mut buf).await.unwrap();
        udp_echo.send_to(&buf[..n], source).await.unwrap();
    });
    let (_control, relay) =
        tokio::time::timeout(STEP_TIMEOUT, socks5_client::udp_associate(inbound))
            .await
            .expect("入站 UDP 关联超时")
            .expect("入站 UDP 关联失败");
    let udp_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let target = Target::Ip("127.0.0.1".parse().unwrap(), udp_port);
    let packet = socks5_client::encode_udp(&target, b"pong").unwrap();
    udp_socket.send_to(&packet, relay).await.unwrap();
    let mut buf = [0u8; 65535];
    let (n, _) = tokio::time::timeout(STEP_TIMEOUT, udp_socket.recv_from(&mut buf))
        .await
        .expect("入站 UDP 读取超时")
        .unwrap();
    let (source, data) = socks5_client::decode_udp(&buf[..n]).unwrap();
    assert_eq!(source, target);
    assert_eq!(data, b"pong");
    let active = state.active.snapshot();
    assert!(
        active.iter().any(|item| {
            item.host == "127.0.0.1"
                && item.port == udp_port
                && item.protocol == ConnectionProtocol::Udp
        }),
        "UDP 目标应出现在活动连接列表：{active:?}"
    );
    udp_task.await.unwrap();

    state.stop().await.unwrap();
    assert!(state.active.snapshot().is_empty());
    let _ = std::fs::remove_dir_all(&config_dir);
}
