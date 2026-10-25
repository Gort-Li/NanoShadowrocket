//! 真实互操作验证（默认跳过）。
//!
//! 需要一个本地运行的官方 v2ray-core 作为服务端：
//! ```text
//! v2ray run -c server.json      # vmess 入站 127.0.0.1:10086，id = b831381d-...，alterId = 0
//! python -m http.server 18080 --bind 127.0.0.1 --directory www
//! www/nano.txt = "NANO-VMESS-OK"，www/big.bin = 128KB 随机数据
//! ```
//! 然后执行：`cargo test --test vmess_e2e -- --ignored --test-threads=1`
//!
//! 所有网络操作都有 6 秒硬超时，任何一步卡住都会快速失败而不是挂起。

use std::time::Duration;

use app_lib::proxy::{Outbound, Target, VmessOutbound};
use app_lib::vmess::{CipherMethod, VmessConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const STEP_TIMEOUT: Duration = Duration::from_secs(6);

fn config(method: CipherMethod) -> VmessConfig {
    VmessConfig {
        host: "127.0.0.1".into(),
        port: 10086,
        uuid: "b831381d-6324-4d53-ad4f-8cda48b30811".into(),
        alter_id: 0,
        security: method,
    }
}

async fn http_get(method: CipherMethod, target: Target, path: &str) -> Vec<u8> {
    let outbound = VmessOutbound::new(config(method));
    let mut stream = tokio::time::timeout(STEP_TIMEOUT, outbound.connect(&target))
        .await
        .expect("握手超时")
        .expect("VMess 握手失败");

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        target.authority()
    );
    tokio::time::timeout(STEP_TIMEOUT, stream.write_all(request.as_bytes()))
        .await
        .expect("写请求超时")
        .unwrap();
    tokio::time::timeout(STEP_TIMEOUT, stream.flush())
        .await
        .expect("flush 超时")
        .unwrap();

    let mut buf = Vec::new();
    tokio::time::timeout(STEP_TIMEOUT, stream.read_to_end(&mut buf))
        .await
        .expect("读响应超时")
        .unwrap();
    buf
}

#[tokio::test]
#[ignore = "需要本地 v2ray-core 服务端（见文件头说明）"]
async fn handshake_aes() {
    let response = http_get(
        CipherMethod::Aes128Gcm,
        Target::parse("127.0.0.1", 18080),
        "/nano.txt",
    )
    .await;
    let text = String::from_utf8_lossy(&response);
    assert!(text.contains("NANO-VMESS-OK"), "AES 失败: {text}");
}

#[tokio::test]
#[ignore = "需要本地 v2ray-core 服务端（见文件头说明）"]
async fn handshake_chacha_and_domain_target() {
    let response = http_get(
        CipherMethod::ChaCha20Poly1305,
        Target::Domain("localhost".into(), 18080),
        "/nano.txt",
    )
    .await;
    let text = String::from_utf8_lossy(&response);
    assert!(text.contains("NANO-VMESS-OK"), "ChaCha/域名 失败: {text}");
}

#[tokio::test]
#[ignore = "需要本地 v2ray-core 服务端（见文件头说明）"]
async fn streams_large_payload_in_multiple_chunks() {
    let response = http_get(
        CipherMethod::Aes128Gcm,
        Target::parse("127.0.0.1", 18080),
        "/big.bin",
    )
    .await;
    let body_start = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("响应缺少头部")
        + 4;
    let body = &response[body_start..];
    assert!(body.len() > 100_000, "大文件长度异常: {}", body.len());
}
