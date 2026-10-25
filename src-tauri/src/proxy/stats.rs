//! 流量统计与活动连接记录。
//!
//! 计数全部使用原子操作，转发线程只在每次 read 之后累加，不参与格式化；
//! 速率由 1 秒一次的采样任务计算，前端只负责渲染。
//! 活动连接用 RAII guard 维护：连接任务结束时自动移除，不需要扫描超时。

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;

use crate::rules::Policy;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficSnapshot {
    pub up_total: u64,
    pub down_total: u64,
    pub up_rate: u64,
    pub down_rate: u64,
    pub active: u64,
    pub total_connections: u64,
}

struct Sample {
    at: Instant,
    up: u64,
    down: u64,
}

pub struct TrafficStats {
    up: AtomicU64,
    down: AtomicU64,
    up_rate: AtomicU64,
    down_rate: AtomicU64,
    active: AtomicI64,
    total: AtomicU64,
    sample: Mutex<Sample>,
}

impl Default for TrafficStats {
    fn default() -> Self {
        Self::new()
    }
}

impl TrafficStats {
    pub fn new() -> Self {
        Self {
            up: AtomicU64::new(0),
            down: AtomicU64::new(0),
            up_rate: AtomicU64::new(0),
            down_rate: AtomicU64::new(0),
            active: AtomicI64::new(0),
            total: AtomicU64::new(0),
            sample: Mutex::new(Sample {
                at: Instant::now(),
                up: 0,
                down: 0,
            }),
        }
    }

    pub fn add_up(&self, bytes: u64) {
        self.up.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn add_down(&self, bytes: u64) {
        self.down.fetch_add(bytes, Ordering::Relaxed);
    }

    /// 新连接开始，返回的 guard 在连接结束时自动减掉活动数。
    pub fn enter(&self) -> ActiveGuard<'_> {
        self.active.fetch_add(1, Ordering::Relaxed);
        self.total.fetch_add(1, Ordering::Relaxed);
        ActiveGuard { stats: self }
    }

    /// 每秒调用一次：用「本秒增量 / 实际间隔」算速率，避免抖动。
    pub fn sample(&self) {
        let up = self.up.load(Ordering::Relaxed);
        let down = self.down.load(Ordering::Relaxed);
        let Ok(mut sample) = self.sample.lock() else {
            return;
        };
        let elapsed = sample.at.elapsed().as_secs_f64();
        if elapsed < 0.2 {
            return;
        }
        let up_rate = ((up.saturating_sub(sample.up)) as f64 / elapsed) as u64;
        let down_rate = ((down.saturating_sub(sample.down)) as f64 / elapsed) as u64;
        sample.at = Instant::now();
        sample.up = up;
        sample.down = down;
        self.up_rate.store(up_rate, Ordering::Relaxed);
        self.down_rate.store(down_rate, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> TrafficSnapshot {
        TrafficSnapshot {
            up_total: self.up.load(Ordering::Relaxed),
            down_total: self.down.load(Ordering::Relaxed),
            up_rate: self.up_rate.load(Ordering::Relaxed),
            down_rate: self.down_rate.load(Ordering::Relaxed),
            active: self.active.load(Ordering::Relaxed).max(0) as u64,
            total_connections: self.total.load(Ordering::Relaxed),
        }
    }

    /// 停止代理时清零，下次会话从 0 开始。
    pub fn reset(&self) {
        self.up.store(0, Ordering::Relaxed);
        self.down.store(0, Ordering::Relaxed);
        self.up_rate.store(0, Ordering::Relaxed);
        self.down_rate.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        if let Ok(mut sample) = self.sample.lock() {
            sample.at = Instant::now();
            sample.up = 0;
            sample.down = 0;
        }
    }
}

pub struct ActiveGuard<'a> {
    stats: &'a TrafficStats,
}

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.stats.active.fetch_sub(1, Ordering::Relaxed);
    }
}

/// 连接使用的传输层协议，前端用于展示 TCP / UDP 标记。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionProtocol {
    Tcp,
    Udp,
}

/// 当前仍然打开的一条连接。UDP 关联按目标去重，关联结束时统一移除。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveConnection {
    pub id: u64,
    pub host: String,
    pub port: u16,
    pub policy: Policy,
    pub rule: Option<String>,
    /// Unix 秒
    pub started_at: u64,
    pub protocol: ConnectionProtocol,
}

/// 活动连接表。每次连接开始创建一个 guard，guard 析构时从表中删除。
pub struct ActiveConnections {
    next_id: AtomicU64,
    entries: Mutex<HashMap<u64, ActiveConnection>>,
}

impl Default for ActiveConnections {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveConnections {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn begin(
        &self,
        host: String,
        port: u16,
        policy: Policy,
        rule: Option<String>,
        protocol: ConnectionProtocol,
    ) -> ConnectionGuard<'_> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let entry = ActiveConnection {
            id,
            host,
            port,
            policy,
            rule,
            started_at: crate::rules::now_secs(),
            protocol,
        };
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(id, entry);
        }
        ConnectionGuard { table: self, id }
    }

    /// 按开始时间稳定排序，前端再做策略 / 主域名分组。
    pub fn snapshot(&self) -> Vec<ActiveConnection> {
        self.entries
            .lock()
            .map(|entries| {
                let mut items: Vec<_> = entries.values().cloned().collect();
                items.sort_by_key(|entry| (entry.started_at, entry.id));
                items
            })
            .unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }
}

pub struct ConnectionGuard<'a> {
    table: &'a ActiveConnections,
    id: u64,
}

impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut entries) = self.table.entries.lock() {
            entries.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_bytes_and_connections() {
        let stats = TrafficStats::new();
        stats.add_up(100);
        stats.add_down(2048);
        {
            let _guard = stats.enter();
            assert_eq!(stats.snapshot().active, 1);
        }
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.up_total, 100);
        assert_eq!(snapshot.down_total, 2048);
        assert_eq!(snapshot.active, 0);
        assert_eq!(snapshot.total_connections, 1);
    }

    #[test]
    fn rate_is_zero_until_sampled_twice() {
        let stats = TrafficStats::new();
        stats.sample();
        stats.add_up(10_000);
        // 采样窗口至少 200ms，保证速率可计算而不是被抖动过滤。
        std::thread::sleep(std::time::Duration::from_millis(250));
        stats.sample();
        assert!(stats.snapshot().up_rate > 0);
    }

    #[test]
    fn reset_clears_session() {
        let stats = TrafficStats::new();
        stats.add_up(1);
        stats.add_down(1);
        stats.reset();
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.up_total, 0);
        assert_eq!(snapshot.down_total, 0);
    }

    #[test]
    fn active_connections_are_removed_when_guards_drop() {
        let table = ActiveConnections::new();
        let first = table.begin(
            "a.example.com".into(),
            443,
            Policy::Proxy,
            Some("DOMAIN-SUFFIX,example.com,Proxy".into()),
            ConnectionProtocol::Tcp,
        );
        let second = table.begin(
            "b.example.com".into(),
            443,
            Policy::Direct,
            None,
            ConnectionProtocol::Udp,
        );
        assert_eq!(table.snapshot().len(), 2);
        drop(first);
        let items = table.snapshot();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].host, "b.example.com");
        assert_eq!(items[0].protocol, ConnectionProtocol::Udp);
        drop(second);
        assert!(table.snapshot().is_empty());
    }

    #[test]
    fn clear_removes_all_active_connections() {
        let table = ActiveConnections::new();
        let _guard = table.begin(
            "example.com".into(),
            443,
            Policy::Proxy,
            None,
            ConnectionProtocol::Tcp,
        );
        table.clear();
        assert!(table.snapshot().is_empty());
    }
}
