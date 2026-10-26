//! 路由/规则匹配性能基准。
//!
//! 覆盖三类真实场景：
//! - `decide_domain_direct` — 纯规则匹配（绕过 Router 缓存），测匹配引擎本身的 CPU。
//! - `decide_cache_hit`    — 反复命中域名的路由缓存，日常高频流量就是这一层。
//! - `decide_cache_miss`   — 不断用新域名（冷缓存），每次都要触发完整匹配 + 写缓存。
//!
//! 模拟 Johnshall 十万行级广告规则集规模：数万条 DOMAIN-SUFFIX + DOMAIN-KEYWORD
//! + 若干 IP-CIDR（no-resolve，避免基准触发真实 DNS）。

use std::sync::Arc;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use app_lib::proxy::dns::{DnsResolver, DnsSetting};
use app_lib::proxy::router::{Router, RouterHandle};
use app_lib::proxy::target::Target;
use app_lib::rules::{build_ruleset, Policy, SourceText};
use tokio::runtime::Runtime;

/// 生成接近生产规模的规则文本：约 5 万条域名后缀黑名单 + 关键词 + IP 规则。
fn big_ruleset_wordlist() -> String {
    let mut out = String::with_capacity(4 << 20);
    for i in 0..50_000u32 {
        out.push_str(&format!(
            "DOMAIN-SUFFIX,ad{i}.example{}.net,Reject\n",
            i % 1000
        ));
    }
    for i in 0..500u32 {
        out.push_str(&format!("DOMAIN-KEYWORD,tracker{i},Reject\n"));
    }
    out.push_str("IP-CIDR,203.0.113.0/24,Direct,no-resolve\n");
    out.push_str("IP-CIDR,198.51.100.0/24,Proxy,no-resolve\n");
    out
}

fn setup_router() -> Arc<Router> {
    let rules = build_ruleset(&[SourceText::custom(big_ruleset_wordlist())], Policy::Proxy);
    let router = Router::new(Arc::new(rules), Arc::new(DnsResolver::new(DnsSetting::System, false)));
    RouterHandle::new(router).get()
}

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime")
}

fn bench_hit(c: &mut Criterion) {
    let handle = setup_router();
    let rt = runtime();
    // 先热一次，确保命中缓存。
    let target = Target::Domain("www.google.com".into(), 443);
    rt.block_on(async { black_box(handle.decide(&target).await) });
    c.bench_function("decide_cache_hit", |b| {
        b.iter(|| {
            rt.block_on(async { black_box(handle.decide(&target).await) });
        });
    });
}

fn bench_miss(c: &mut Criterion) {
    let handle = setup_router();
    let rt = runtime();
    let mut seq = 0u32;
    c.bench_function("decide_cache_miss", |b| {
        b.iter_custom(|iters| {
            let start = std::time::Instant::now();
            for _ in 0..iters {
                seq = seq.wrapping_add(1);
                let target = Target::Domain(format!("fresh-{seq}.example.com"), 443);
                rt.block_on(async { black_box(handle.decide(&target).await) });
            }
            start.elapsed()
        });
    });
}

fn bench_decide_domain(c: &mut Criterion) {
    let rules = build_ruleset(&[SourceText::custom(big_ruleset_wordlist())], Policy::Proxy);
    c.bench_function("decide_domain_direct", |b| {
        b.iter(|| {
            black_box(
                rules.decide_domain(black_box("ads.google-analytics.example.com")),
            )
        });
    });
}

fn bench_multithread_hit(c: &mut Criterion) {
    let handle = setup_router();
    // 预填充一个足够大的域名池，模拟真实流量的长尾分布（各线程命中不同缓存键）。
    const POOL: usize = 4096;
    let targets: Vec<Target> = (0..POOL)
        .map(|i| Target::Domain(format!("site{i}.example.com"), 443))
        .collect();
    let rt = runtime();
    black_box(rt.block_on(async {
        for t in &targets {
            black_box(handle.decide(t).await);
        }
    }));
    let shared = Arc::new((handle, targets));
    c.bench_function("decide_cache_hit_parallel_8", |b| {
        b.iter_custom(|_iters| {
            // 8 个线程并发命中不同缓存键，量化锁竞争对真实长尾流量的影响。
            let threads: Vec<_> = (0..8)
                .map(|tid| {
                    let shared = Arc::clone(&shared);
                    std::thread::spawn(move || {
                        let (handle, targets) = &*shared;
                        let rt = runtime();
                        let start = std::time::Instant::now();
                        let mut i = tid * (POOL / 8);
                        for n in 0..40_000 {
                            let target = &targets[i];
                            rt.block_on(async { black_box(handle.decide(target).await) });
                            i = (i + 1) % POOL;
                            black_box(n);
                        }
                        start.elapsed()
                    })
                })
                .collect();
            let mut max_elapsed = Duration::ZERO;
            for t in threads {
                let elapsed = t.join().expect("thread join");
                max_elapsed = max_elapsed.max(elapsed);
            }
            max_elapsed
        });
    });
}

criterion_group!(
    name = route;
    config = Criterion::default().warm_up_time(Duration::from_millis(200)).measurement_time(Duration::from_secs(2)).sample_size(100);
    targets = bench_decide_domain, bench_hit, bench_miss, bench_multithread_hit
);
criterion_main!(route);