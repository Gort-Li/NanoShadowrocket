//! HTTP 代理请求解析。
//!
//! 只支持两种形态：
//! - `CONNECT host:port HTTP/1.1`（HTTPS / 任意 TCP）
//! - `GET http://host/path HTTP/1.1`（明文 HTTP，请求行会被改写成 origin-form）

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::proxy::target::Target;

const MAX_HEAD: usize = 16 * 1024;

pub struct HttpHead {
    pub target: Target,
    /// 改写后的请求头（明文 HTTP 用），CONNECT 时为空。
    pub forward: Vec<u8>,
    pub is_connect: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("请求头不完整")]
    Incomplete,
    #[error("请求头超过 16KB")]
    TooLarge,
    #[error("不是受支持的 HTTP 代理请求")]
    BadRequest,
    #[error("I/O 错误：{0}")]
    Io(#[from] std::io::Error),
}

/// 读取一个请求头，返回解析结果与「多读到的 body 字节」。
pub async fn read_request(stream: &mut TcpStream) -> Result<(HttpHead, Vec<u8>), HttpError> {
    let mut buf = Vec::with_capacity(1024);
    let mut tmp = [0u8; 2048];
    let head_end = loop {
        if let Some(pos) = find_head_end(&buf) {
            break pos;
        }
        if buf.len() > MAX_HEAD {
            let _ = reply_error(stream, 431, "请求头过大").await;
            return Err(HttpError::TooLarge);
        }
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(HttpError::Incomplete);
        }
        buf.extend_from_slice(&tmp[..n]);
    };

    let extra = buf[head_end..].to_vec();
    let head_text = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let uri = parts.next().unwrap_or_default().to_string();
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();

    if method.is_empty() || uri.is_empty() {
        let _ = reply_error(stream, 400, "请求行格式错误").await;
        return Err(HttpError::BadRequest);
    }

    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = split_authority(&uri, 443).ok_or(HttpError::BadRequest)?;
        return Ok((
            HttpHead {
                target: Target::parse(host, port),
                forward: Vec::new(),
                is_connect: true,
            },
            extra,
        ));
    }

    let Some((scheme, rest)) = uri.split_once("://") else {
        let _ = reply_error(stream, 400, "请通过代理发送绝对 URI，或使用 CONNECT").await;
        return Err(HttpError::BadRequest);
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        let _ = reply_error(stream, 400, "不支持的协议，仅支持 http/https").await;
        return Err(HttpError::BadRequest);
    }
    let default_port = if scheme.eq_ignore_ascii_case("https") {
        443
    } else {
        80
    };
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    let (host, port) = split_authority(authority, default_port).ok_or(HttpError::BadRequest)?;

    let mut forward = Vec::with_capacity(head_text.len());
    forward.extend_from_slice(format!("{method} {path} {version}\r\n").as_bytes());
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("proxy-connection:") || lower.starts_with("proxy-authorization:") {
            continue;
        }
        forward.extend_from_slice(line.as_bytes());
        forward.extend_from_slice(b"\r\n");
    }
    forward.extend_from_slice(b"\r\n");

    Ok((
        HttpHead {
            target: Target::parse(host, port),
            forward,
            is_connect: false,
        },
        extra,
    ))
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|pos| pos + 4)
}

/// 解析 `host:port` / `[::1]:443` / `host`（用默认端口）。
fn split_authority(text: &str, default_port: u16) -> Option<(&str, u16)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(rest) = text.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &text[..end + 2];
        let tail = &text[end + 2..];
        let port = match tail.strip_prefix(':') {
            Some(port) => port.parse().ok()?,
            None if tail.is_empty() => default_port,
            None => return None,
        };
        return Some((host, port));
    }
    match text.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') && !host.is_empty() => {
            Some((host, port.parse().ok()?))
        }
        Some(_) => None,
        None => Some((text, default_port)),
    }
}

pub async fn reply_connect_ok(stream: &mut TcpStream) -> std::io::Result<()> {
    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\nProxy-Agent: NanoShadowrocket\r\n\r\n")
        .await?;
    stream.flush().await
}

pub async fn reply_error(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = match status {
        400 => "Bad Request",
        403 => "Forbidden",
        431 => "Request Header Fields Too Large",
        502 => "Bad Gateway",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_authorities() {
        assert_eq!(split_authority("a.com:8080", 80), Some(("a.com", 8080)));
        assert_eq!(split_authority("a.com", 80), Some(("a.com", 80)));
        assert_eq!(split_authority("[::1]:443", 80), Some(("[::1]", 443)));
        assert_eq!(split_authority("::1", 80), None);
        assert_eq!(split_authority("a.com:0", 80), Some(("a.com", 0)));
    }

    #[test]
    fn finds_header_end() {
        assert_eq!(find_head_end(b"GET / HTTP/1.1\r\n\r\nbody"), Some(18));
        assert_eq!(find_head_end(b"GET / HTTP/1.1\r\n"), None);
    }
}
