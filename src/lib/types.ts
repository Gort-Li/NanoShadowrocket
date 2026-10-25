export type Policy = "proxy" | "direct" | "reject";

export type Transport = "tcp" | "ws" | "grpc" | "h2" | "quic" | "kcp" | "http";

export type Security = "aes-128-gcm" | "chacha20-poly1305";

export type RemoteStatus = "idle" | "ok" | "error";

export interface EnvValue {
  value: string;
  expand: boolean;
}

export interface EnvSnapshot {
  vars: Record<string, EnvValue>;
}

export interface ProxySnapshot {
  proxyEnable: number;
  proxyServer: string;
  proxyOverride: string;
  autoConfigUrl?: string | null;
}

export interface Node {
  id: string;
  name: string;
  transport: Transport;
  host: string;
  port: number;
  uuid: string;
  alterId: number;
  security: Security;
  latencyMs: number | null;
}

export interface RemoteRule {
  id: string;
  name: string;
  url: string;
  intervalHours: number;
  enabled: boolean;
  etag: string | null;
  lastModified: string | null;
  updatedAt: number | null;
  ruleCount: number;
  ignoredCount: number;
  status: RemoteStatus;
  error: string | null;
}

export interface AppConfig {
  listenPort: number;
  allowLan: boolean;
  defaultPolicy: Policy;
  systemProxy: boolean;
  proxyEnv: boolean;
  launchAtStartup: boolean;
  dnsServer: string;
  ipv6: boolean;
  nodes: Node[];
  selectedNode: string | null;
  customRules: string;
  remoteRules: RemoteRule[];
  proxyDirty: boolean;
  proxySnapshot: ProxySnapshot | null;
  envDirty: boolean;
  envSnapshot: EnvSnapshot | null;
}

export interface ProxyStatus {
  running: boolean;
  localPort: number;
  nodeId: string | null;
  nodeName: string | null;
  startedAt: number | null;
  systemProxy: boolean;
  envProxy: boolean;
  defaultPolicy: Policy;
  ruleCount: number;
  remoteSources: number;
  allowLan: boolean;
}

export interface RulesSummary {
  total: number;
  custom: number;
  remote: number;
  ignored: number;
  byKind: Record<string, number>;
  ignoredByReason: Record<string, number>;
  defaultPolicy: Policy;
}

export interface HistoryPoint {
  up: number;
  down: number;
}

export interface TrafficStats {
  upTotal: number;
  downTotal: number;
  upRate: number;
  downRate: number;
  active: number;
  totalConnections: number;
}

export interface ConnectionEntry {
  host: string;
  port: number;
  policy: Policy;
  rule: string | null;
  at: number;
}

export interface Bootstrap {
  config: AppConfig;
  rules: RulesSummary;
  status: ProxyStatus;
  stats: TrafficStats;
  recent: ConnectionEntry[];
}

export interface RuleTestResult {
  input: string;
  matched: boolean;
  policy: Policy;
  defaultPolicy: Policy;
  rule: string | null;
  ruleLabel: string | null;
  ruleLine: number | null;
  source: string | null;
  resolvedIp: string | null;
  totalRules: number;
}

export interface LatencyResult {
  id: string;
  name: string;
  latencyMs: number | null;
  error: string | null;
}

export interface RemoteOutcome {
  id: string;
  changed: boolean;
  ruleCount: number;
  ignoredCount: number;
  error: string | null;
}

export interface NodeInput {
  id?: string | null;
  name: string;
  host: string;
  port: number;
  uuid: string;
  alterId: number;
  security: Security;
  transport: Transport;
}

export interface SettingsInput {
  listenPort: number;
  allowLan: boolean;
  defaultPolicy: Policy;
  systemProxy: boolean;
  proxyEnv: boolean;
  launchAtStartup: boolean;
  dnsServer: string;
  ipv6: boolean;
}

export interface TrafficPayload {
  stats: TrafficStats;
  running: boolean;
  recent: ConnectionEntry[];
}

export interface AddNodeResult {
  node: Node;
  warnings: string[];
}
