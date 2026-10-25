//! 真实订阅解析验证（默认跳过，需要访问 GitHub Pages）。
//!
//! 用 Johnshall 的两个 top500 规则集校验：解析容忍度、编译耗时、典型域名决策。
//! 运行：`cargo test --test rules_real -- --ignored --nocapture`

use std::time::{Duration, Instant};

use app_lib::rules::{build_ruleset, parse_conf, Policy, RuleSet, SourceText};

const BANLIST: &str =
    "https://johnshall.github.io/Shadowrocket-ADBlock-Rules-Forever/sr_top500_banlist_ad.conf";
const WHITELIST: &str =
    "https://johnshall.github.io/Shadowrocket-ADBlock-Rules-Forever/sr_top500_whitelist_ad.conf";

async fn fetch(url: &str) -> String {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    client
        .get(url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap()
}

fn compile(label: &str, text: String, default: Policy) -> RuleSet {
    let parsed = parse_conf(&text);
    println!(
        "[{label}] bytes={} lines={} rules={} ignored={:?}",
        text.len(),
        parsed.lines,
        parsed.rules.len(),
        parsed
            .ignored
            .iter()
            .map(|i| i.reason.to_string())
            .collect::<Vec<_>>()
    );

    let started = Instant::now();
    let set = build_ruleset(
        &[SourceText::remote(
            label.to_string(),
            label.to_string(),
            text,
        )],
        default,
    );
    println!(
        "[{label}] compiled={:?} total={} stats={:?}",
        started.elapsed(),
        set.len(),
        set.stats()
    );
    set
}

fn report(label: &str, set: &RuleSet, hosts: &[&str]) {
    for host in hosts {
        let decision = set.decide_domain(host);
        let rule = decision
            .rule
            .and_then(|idx| set.rule(idx))
            .map(|m| m.raw.as_str())
            .unwrap_or("<default>");
        println!(
            "[{label}] {host} -> {:?} needs_resolve={} rule={rule}",
            decision.policy, decision.needs_resolve
        );
    }
}

#[tokio::test]
#[ignore = "需要访问 GitHub Pages"]
async fn banlist_parses_and_matches() {
    let text = fetch(BANLIST).await;
    let set = compile("banlist", text, Policy::Direct);
    assert!(set.len() > 80_000, "规则条数异常: {}", set.len());

    report(
        "banlist",
        &set,
        &[
            "github.com",
            "www.baidu.com",
            "doubleclick.net",
            "example.org",
            "www.qq.com",
        ],
    );
}

#[tokio::test]
#[ignore = "需要访问 GitHub Pages"]
async fn whitelist_parses_and_matches() {
    let text = fetch(WHITELIST).await;
    let set = compile("whitelist", text, Policy::Proxy);
    assert!(set.len() > 50_000, "规则条数异常: {}", set.len());

    report(
        "whitelist",
        &set,
        &[
            "github.com",
            "www.baidu.com",
            "doubleclick.net",
            "www.sina.com.cn",
            "example.org",
        ],
    );
}
