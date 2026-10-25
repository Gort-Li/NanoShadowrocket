//! 流量统计与最近连接记录。
//!
//! 计数全部使用原子操作，转发线程只在每次 read 之后累加，不参与格式化；
//! 速率由 1 秒一次的采样任务计算，前端只负责渲染。

use std::collections::VecDeque;
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

/// 最近连接记录中的一条。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionEntry {
    pub host: String,
    pub port: u16,
    pub policy: Policy,
    pub rule: Option<String>,
    /// Unix 秒
    pub at: u64,
}

/// 固定容量的环形缓冲区，保留最近 N 条连接。
pub struct ConnectionLog {
    limit: usize,
    entries: Mutex<VecDeque<ConnectionEntry>>,
}

impl ConnectionLog {
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            entries: Mutex::new(VecDeque::with_capacity(limit.min(32))),
        }
    }

    pub fn push(&self, entry: ConnectionEntry) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.push_front(entry);
            while entries.len() > self.limit {
                entries.pop_back();
            }
        }
    }

    pub fn snapshot(&self) -> Vec<ConnectionEntry> {
        self.entries
            .lock()
            .map(|entries| entries.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(host: &str) -> ConnectionEntry {
        ConnectionEntry {
            host: host.into(),
            port: 443,
            policy: Policy::Proxy,
            rule: Some("DOMAIN-SUFFIX,example.com,Proxy".into()),
            at: 1_700_000_000,
        }
    }

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
    fn recent_log_is_bounded_and_newest_first() {
        let log = ConnectionLog::new(3);
        for i in 0..5 {
            log.push(entry(&format!("h{i}.com")));
        }
        let items = log.snapshot();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].host, "h4.com");
        log.clear();
        assert!(log.snapshot().is_empty());
    }
}
