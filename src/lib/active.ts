import type { ActiveConnection, ConnectionProtocol, Policy } from "./types";

const MULTI_LEVEL_SUFFIXES = new Set([
  "com.cn",
  "net.cn",
  "org.cn",
  "gov.cn",
  "edu.cn",
  "com.hk",
  "com.tw",
  "com.au",
  "com.sg",
  "com.br",
  "com.mx",
  "co.uk",
  "org.uk",
  "ac.uk",
  "co.jp",
  "co.kr",
  "co.in",
  "co.nz",
  "co.za",
]);

const POLICY_ORDER: Record<Policy, number> = {
  proxy: 0,
  direct: 1,
  reject: 2,
};

export interface ActiveHostGroup {
  host: string;
  ports: number[];
  protocols: ConnectionProtocol[];
  policy: Policy;
  mixed: boolean;
  count: number;
  latest: number;
  rule: string | null;
}

export interface ActiveDomainGroup {
  domain: string;
  policy: Policy;
  mixed: boolean;
  count: number;
  latest: number;
  hosts: ActiveHostGroup[];
}

export function isIpHost(host: string): boolean {
  const value = host.trim().replace(/^\[|\]$/g, "");
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(value) || value.includes(":");
}

export function normalizeHost(host: string): string {
  return host.trim().replace(/^\[|\]$/g, "").replace(/\.$/, "").toLowerCase();
}

/** 不使用完整 PSL，覆盖常见的二级后缀即可；IP 与单标签域名原样返回。 */
export function mainDomain(host: string): string {
  const value = normalizeHost(host);
  if (!value || isIpHost(value)) return value;

  const labels = value.split(".").filter(Boolean);
  if (labels.length <= 2) return value;

  const lastTwo = labels.slice(-2).join(".");
  if (MULTI_LEVEL_SUFFIXES.has(lastTwo)) {
    return labels.slice(-3).join(".");
  }
  return lastTwo;
}

function bestPolicy(policies: Iterable<Policy>): { policy: Policy; mixed: boolean } {
  const unique = new Set(policies);
  let policy: Policy | null = null;
  for (const item of unique) {
    if (!policy || POLICY_ORDER[item] < POLICY_ORDER[policy]) policy = item;
  }
  return { policy: policy ?? "proxy", mixed: unique.size > 1 };
}

/** 把逐条活动连接聚合成「主域名 → 主机 → 端口」树，并按策略排序。 */
export function groupActiveConnections(entries: ActiveConnection[]): ActiveDomainGroup[] {
  const domains = new Map<
    string,
    Map<
      string,
      {
        ports: Set<number>;
        protocols: Set<ConnectionProtocol>;
        policies: Policy[];
        count: number;
        latest: number;
        rule: string | null;
      }
    >
  >();

  for (const entry of entries) {
    const host = normalizeHost(entry.host);
    if (!host) continue;
    const domain = mainDomain(host);
    let hosts = domains.get(domain);
    if (!hosts) {
      hosts = new Map();
      domains.set(domain, hosts);
    }
    let item = hosts.get(host);
    if (!item) {
      item = {
        ports: new Set(),
        protocols: new Set(),
        policies: [],
        count: 0,
        latest: 0,
        rule: null,
      };
      hosts.set(host, item);
    }
    item.ports.add(entry.port);
    item.protocols.add(entry.protocol);
    item.policies.push(entry.policy);
    item.count += 1;
    if (entry.startedAt >= item.latest) {
      item.latest = entry.startedAt;
      item.rule = entry.rule;
    }
  }

  const groups: ActiveDomainGroup[] = [];
  for (const [domain, hostMap] of domains) {
    const hosts: ActiveHostGroup[] = [];
    for (const [host, item] of hostMap) {
      const best = bestPolicy(item.policies);
      hosts.push({
        host,
        ports: [...item.ports].sort((a, b) => a - b),
        protocols: [...item.protocols].sort(),
        policy: best.policy,
        mixed: best.mixed,
        count: item.count,
        latest: item.latest,
        rule: item.rule,
      });
    }

    hosts.sort((a, b) => {
      const byPolicy = POLICY_ORDER[a.policy] - POLICY_ORDER[b.policy];
      if (byPolicy !== 0) return byPolicy;
      if (a.count !== b.count) return b.count - a.count;
      return a.host.localeCompare(b.host);
    });

    const best = bestPolicy(hosts.map((host) => host.policy));
    groups.push({
      domain,
      policy: best.policy,
      // 子主机内部混合（同一主机同时命中代理和直连）也要在组上标出来。
      mixed: best.mixed || hosts.some((host) => host.mixed),
      count: hosts.reduce((sum, host) => sum + host.count, 0),
      latest: hosts.reduce((latest, host) => Math.max(latest, host.latest), 0),
      hosts,
    });
  }

  groups.sort((a, b) => {
    const byPolicy = POLICY_ORDER[a.policy] - POLICY_ORDER[b.policy];
    if (byPolicy !== 0) return byPolicy;
    if (a.count !== b.count) return b.count - a.count;
    return a.domain.localeCompare(b.domain);
  });

  return groups;
}
