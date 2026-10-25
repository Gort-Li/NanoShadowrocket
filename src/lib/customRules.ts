import { isIpHost, normalizeHost } from "./active";
import type { Policy } from "./types";

export type CustomRuleKind =
  | "DOMAIN"
  | "DOMAIN-SUFFIX"
  | "DOMAIN-KEYWORD"
  | "IP-CIDR"
  | "GEOIP"
  | "FINAL";

export interface CustomRule {
  kind: CustomRuleKind;
  value: string;
  policy: Policy;
  noResolve: boolean;
}

export interface ParsedCustomRules {
  rules: CustomRule[];
  /** 表单不支持的原始行；保存时原样保留，避免静默丢规则。 */
  unsupported: string[];
}

const KINDS = new Set<CustomRuleKind>([
  "DOMAIN",
  "DOMAIN-SUFFIX",
  "DOMAIN-KEYWORD",
  "IP-CIDR",
  "GEOIP",
  "FINAL",
]);

const POLICY_ALIASES: Record<string, Policy> = {
  proxy: "proxy",
  allow: "proxy",
  direct: "direct",
  bypass: "direct",
  reject: "reject",
  deny: "reject",
  block: "reject",
};

const DOMAIN_RE =
  /^(?=.{1,253}$)(?:[a-z0-9_](?:[a-z0-9_-]{0,61}[a-z0-9_])?\.)*[a-z0-9_](?:[a-z0-9_-]{0,61}[a-z0-9_])?$/i;

export function policyName(policy: Policy): "Proxy" | "Direct" | "Reject" {
  return policy === "proxy" ? "Proxy" : policy === "direct" ? "Direct" : "Reject";
}

export function parsePolicy(raw: string | undefined): Policy | null {
  return POLICY_ALIASES[(raw ?? "").trim().toLowerCase()] ?? null;
}

export function isCustomRuleKind(value: string): value is CustomRuleKind {
  return KINDS.has(value as CustomRuleKind);
}

function normalizeValue(kind: CustomRuleKind, raw: string): string {
  if (kind === "GEOIP") return raw.trim().toUpperCase();
  if (kind === "IP-CIDR") return raw.trim();
  return normalizeHost(raw).replace(/^\*\./, "");
}

function isValidIpv4Cidr(value: string): boolean {
  const [ip, prefix, ...rest] = value.split("/");
  if (rest.length > 0 || !ip || !prefix) return false;
  const parts = ip.split(".");
  if (parts.length !== 4) return false;
  if (!parts.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255)) return false;
  return /^\d{1,2}$/.test(prefix) && Number(prefix) <= 32;
}

function isValidIpv6Cidr(value: string): boolean {
  const [ip, prefix, ...rest] = value.split("/");
  if (rest.length > 0 || !ip || !prefix) return false;
  if (!ip.includes(":") || !/^[0-9a-f:]+$/i.test(ip)) return false;
  return /^\d{1,3}$/.test(prefix) && Number(prefix) <= 128;
}

function isValidCidr(value: string): boolean {
  return value.includes(":") ? isValidIpv6Cidr(value) : isValidIpv4Cidr(value);
}

export function validateCustomRule(rule: CustomRule): string | null {
  if (rule.kind === "FINAL") return null;

  const value = rule.value.trim();
  if (!value) return "请填写匹配内容";

  if (rule.kind === "DOMAIN" || rule.kind === "DOMAIN-SUFFIX") {
    if (!DOMAIN_RE.test(value)) return "域名格式不正确，例如 api.example.com";
    return null;
  }
  if (rule.kind === "DOMAIN-KEYWORD") {
    if (/[\s,]/.test(value)) return "关键词不能包含空格或逗号";
    return null;
  }
  if (rule.kind === "IP-CIDR") {
    if (!isValidCidr(value)) return "CIDR 格式不正确，例如 203.0.113.0/24";
    return null;
  }
  if (rule.kind === "GEOIP") {
    if (!/^[A-Z]{2}$/.test(value)) return "国家/地区代码应为两位字母，例如 CN";
    return null;
  }
  return null;
}

/** 解析自定义规则文本；注释与空行丢弃，无法识别的行留给 unsupported。 */
export function parseCustomRules(text: string): ParsedCustomRules {
  const rules: CustomRule[] = [];
  const unsupported: string[] = [];

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#") || line.startsWith(";") || line.startsWith("//")) {
      continue;
    }

    const fields = line.split(",").map((field) => field.trim());
    const rawKind = (fields[0] ?? "").toUpperCase();
    const kind =
      rawKind === "MATCH" ? "FINAL" : rawKind === "IP-CIDR6" ? "IP-CIDR" : rawKind;
    if (!isCustomRuleKind(kind)) {
      unsupported.push(line);
      continue;
    }

    if (kind === "FINAL") {
      const policy = parsePolicy(fields[1]);
      if (!policy) {
        unsupported.push(line);
        continue;
      }
      rules.push({ kind, value: "", policy, noResolve: false });
      continue;
    }

    const policy = parsePolicy(fields[2]);
    const value = normalizeValue(kind, fields[1] ?? "");
    if (!policy || !value) {
      unsupported.push(line);
      continue;
    }
    const noResolve =
      (kind === "IP-CIDR" || kind === "GEOIP") &&
      fields.slice(3).some((field) => field.toLowerCase() === "no-resolve");
    const rule: CustomRule = { kind, value, policy, noResolve };
    if (validateCustomRule(rule)) {
      unsupported.push(line);
      continue;
    }
    rules.push(rule);
  }

  return { rules, unsupported };
}

export function formatCustomRule(rule: CustomRule): string {
  if (rule.kind === "FINAL") return `FINAL,${policyName(rule.policy)}`;
  const parts = [rule.kind, rule.value, policyName(rule.policy)];
  if (rule.noResolve && (rule.kind === "IP-CIDR" || rule.kind === "GEOIP")) {
    parts.push("no-resolve");
  }
  return parts.join(",");
}

export function serializeCustomRules(rules: CustomRule[], unsupported: string[] = []): string {
  const lines = rules.map(formatCustomRule);
  for (const line of unsupported) {
    if (line.trim()) lines.push(line.trim());
  }
  return lines.length ? `${lines.join("\n")}\n` : "";
}

function ruleKey(rule: CustomRule): string {
  if (rule.kind === "FINAL") return "FINAL";
  return `${rule.kind}:${rule.value.toLowerCase()}`;
}

/** 快速配置用：同类型同值规则更新并提到最前，确保优先级最高。 */
export function upsertCustomRule(rules: CustomRule[], next: CustomRule): CustomRule[] {
  const key = ruleKey(next);
  const rest = rules.filter((rule) => ruleKey(rule) !== key);
  return [next, ...rest];
}

export function ruleForHost(
  host: string,
  policy: Policy,
  scope: "exact" | "suffix"
): CustomRule {
  const value = normalizeHost(host);
  if (isIpHost(value)) {
    const cidr = value.includes(":") ? `${value}/128` : `${value}/32`;
    return { kind: "IP-CIDR", value: cidr, policy, noResolve: false };
  }
  return {
    kind: scope === "suffix" ? "DOMAIN-SUFFIX" : "DOMAIN",
    value,
    policy,
    noResolve: false,
  };
}
