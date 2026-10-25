import type { ActiveConnection, Bootstrap, HistoryPoint, Node, RuleTestResult } from "./types";

const NOW = Math.floor(Date.now() / 1000);

const NODES: Node[] = [
  {
    id: "demo-hk",
    name: "香港 · 01",
    transport: "tcp",
    host: "hk1.nano-edge.net",
    port: 443,
    uuid: "b831381d-6324-4d53-ad4f-8cda48b30811",
    alterId: 0,
    security: "aes-128-gcm",
    udp: true,
    latencyMs: 62,
  },
  {
    id: "demo-jp",
    name: "东京 · 02",
    transport: "tcp",
    host: "jp2.nano-edge.net",
    port: 8443,
    uuid: "9c2f74b1-1f4e-4b7e-9a02-73c4f0aa2e10",
    alterId: 0,
    security: "chacha20-poly1305",
    udp: true,
    latencyMs: 148,
  },
  {
    id: "demo-sg",
    name: "新加坡 · 备用",
    transport: "tcp",
    host: "sg3.nano-edge.net",
    port: 443,
    uuid: "5a61c9e4-2a3b-49fb-8d43-91a0c7be3312",
    alterId: 0,
    security: "aes-128-gcm",
    udp: true,
    latencyMs: null,
  },
];

const ACTIVE: ActiveConnection[] = [
  { id: 1, host: "github.com", port: 443, policy: "proxy", rule: "DOMAIN-SUFFIX,github.com,Proxy", startedAt: NOW - 3725, protocol: "tcp" },
  { id: 2, host: "github.com", port: 22, policy: "proxy", rule: "DOMAIN-SUFFIX,github.com,Proxy", startedAt: NOW - 2810, protocol: "tcp" },
  { id: 3, host: "api.github.com", port: 443, policy: "proxy", rule: "DOMAIN-SUFFIX,github.com,Proxy", startedAt: NOW - 1800, protocol: "tcp" },
  { id: 4, host: "raw.githubusercontent.com", port: 443, policy: "proxy", rule: "DOMAIN-KEYWORD,github,Proxy", startedAt: NOW - 920, protocol: "tcp" },
  { id: 5, host: "api.githubcopilot.com", port: 443, policy: "proxy", rule: "DOMAIN-KEYWORD,github,Proxy", startedAt: NOW - 640, protocol: "tcp" },
  { id: 6, host: "cdn.jsdelivr.net", port: 443, policy: "direct", rule: "DOMAIN-SUFFIX,jsdelivr.net,Direct", startedAt: NOW - 510, protocol: "tcp" },
  { id: 7, host: "www.bilibili.com", port: 443, policy: "direct", rule: "DOMAIN-SUFFIX,bilibili.com,Direct", startedAt: NOW - 320, protocol: "tcp" },
  { id: 8, host: "api.bilibili.com", port: 443, policy: "direct", rule: "DOMAIN-SUFFIX,bilibili.com,Direct", startedAt: NOW - 180, protocol: "tcp" },
  { id: 9, host: "1.1.1.1", port: 53, policy: "direct", rule: "IP-CIDR,1.1.1.1/32,Direct,no-resolve", startedAt: NOW - 90, protocol: "udp" },
];

/** 预览模式下的历史曲线，让首屏就能看到完整形态。 */
export function demoHistory(): HistoryPoint[] {
  return Array.from({ length: 60 }, (_, index) => {
    const wave = Math.sin((index / 60) * Math.PI * 2.2) * 0.35 + 0.65;
    const jitter = 0.82 + ((index * 37) % 23) / 100;
    return {
      up: Math.round(18_000 * wave * jitter),
      down: Math.round(340_000 * wave * jitter + (index % 7) * 9_000),
    };
  });
}

export function demoBootstrap(): Bootstrap {
  return {
    config: {
      listenPort: 8888,
      allowLan: false,
      defaultPolicy: "proxy",
      systemProxy: true,
      proxyEnv: true,
      launchAtStartup: false,
      dnsServer: "https://cloudflare-dns.com/dns-query",
      ipv6: true,
      nodes: NODES,
      selectedNode: NODES[0].id,
      customRules: [
        "# 自定义规则优先级最高，从上到下匹配，命中即止。",
        "DOMAIN-SUFFIX,nano-edge.net,DIRECT",
        "DOMAIN-KEYWORD,doubleclick,REJECT",
        "DOMAIN-SUFFIX,github.com,PROXY",
        "DOMAIN-SUFFIX,jsdelivr.net,DIRECT",
        "IP-CIDR,203.0.113.0/24,DIRECT,no-resolve",
      ].join("\n"),
      remoteRules: [
        {
          id: "demo-remote-1",
          name: "Johnshall 分流规则",
          url: "https://raw.githubusercontent.com/Johnshall/Shadowrocket-ADBlock-Rules-Forever/release/lazy.conf",
          intervalHours: 12,
          enabled: true,
          etag: 'W/"8f1c"',
          lastModified: "Sun, 25 Oct 2026 02:10:00 GMT",
          updatedAt: 1761350400,
          ruleCount: 14820,
          ignoredCount: 36,
          status: "ok",
          error: null,
        },
        {
          id: "demo-remote-2",
          name: "广告拦截合集",
          url: "https://raw.githubusercontent.com/blackmatrix7/ios_rule_script/master/rule/Shadowrocket/Advertising/Advertising.list",
          intervalHours: 24,
          enabled: true,
          etag: null,
          lastModified: null,
          updatedAt: 1761264000,
          ruleCount: 2864,
          ignoredCount: 0,
          status: "ok",
          error: null,
        },
        {
          id: "demo-remote-3",
          name: "自建直连白名单",
          url: "https://rules.nano-edge.net/direct.conf",
          intervalHours: 6,
          enabled: false,
          etag: null,
          lastModified: null,
          updatedAt: null,
          ruleCount: 0,
          ignoredCount: 0,
          status: "idle",
          error: null,
        },
      ],
      proxyDirty: false,
      proxySnapshot: null,
      envDirty: false,
      envSnapshot: null,
    },
    rules: {
      total: 17819,
      custom: 5,
      remote: 17684,
      ignored: 36,
      byKind: {
        DOMAIN: 9621,
        "DOMAIN-SUFFIX": 6120,
        "DOMAIN-KEYWORD": 412,
        "IP-CIDR": 1654,
        GEOIP: 1,
        FINAL: 2,
      },
      ignoredByReason: { "不支持的规则类型 USER-AGENT": 36 },
      defaultPolicy: "proxy",
    },
    status: {
      running: true,
      localPort: 8888,
      nodeId: NODES[0].id,
      nodeName: NODES[0].name,
      startedAt: NOW - 3725,
      systemProxy: true,
      envProxy: true,
      defaultPolicy: "proxy",
      ruleCount: 17819,
      remoteSources: 3,
      allowLan: false,
    },
    stats: {
      upTotal: 12_845_312,
      downTotal: 86_420_117,
      upRate: 18_400,
      downRate: 342_000,
      active: ACTIVE.length,
      totalConnections: 47,
    },
    active: ACTIVE,
  };
}

export function demoRuleTest(input: string): RuleTestResult {
  const value = input.trim().toLowerCase();
  const custom: RuleTestResult = {
    input,
    matched: true,
    policy: "direct",
    defaultPolicy: "proxy",
    rule: "DOMAIN-SUFFIX,nano-edge.net,DIRECT",
    ruleLabel: "DOMAIN-SUFFIX",
    ruleLine: 2,
    source: "自定义规则",
    resolvedIp: null,
    totalRules: 17819,
  };
  if (value.includes("github")) {
    return {
      ...custom,
      policy: "proxy",
      rule: "DOMAIN-SUFFIX,github.com,PROXY",
      ruleLine: 4,
    };
  }
  if (value.includes("doubleclick") || value.includes("ads")) {
    return {
      ...custom,
      policy: "reject",
      rule: "DOMAIN-KEYWORD,doubleclick,REJECT",
      ruleLine: 3,
    };
  }
  if (value.includes("nano-edge") || value.includes("jsdelivr") || value.includes("bilibili")) {
    return custom;
  }
  return {
    input,
    matched: false,
    policy: "proxy",
    defaultPolicy: "proxy",
    rule: null,
    ruleLabel: null,
    ruleLine: null,
    source: null,
    resolvedIp: null,
    totalRules: 17819,
  };
}
