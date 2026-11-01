export type Policy = "proxy" | "direct" | "reject";

export type Protocol = "vmess" | "vless";

export type Transport = "tcp" | "ws" | "grpc" | "h2" | "quic" | "kcp" | "http";

export type Security = "aes-128-gcm" | "chacha20-poly1305" | "legacy";

/**
 * TLS / REALITY 流加密参数（协议无关，vmess/vless 共用）。
 * realityPublicKey 存在且 enabled 时按 REALITY 处理，否则按普通 TLS。
 */
export interface NodeTls {
  enabled: boolean;
  serverName?: string | null;
  fingerprint?: string | null;
  realityPublicKey?: string | null;
  realityShortId?: string | null;
  realitySpiderX?: string | null;
  alpn?: string[] | null;
}

/** WebSocket 传输参数（仅 transport=ws 时生效）。 */
export interface NodeWs {
  path?: string | null;
  host?: string | null;
}

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
  protocol: Protocol;
  transport: Transport;
  host: string;
  port: number;
  uuid: string;
  alterId: number;
  security: Security;
  tls?: NodeTls | null;
  ws?: NodeWs | null;
  flow?: string | null;
  udp: boolean;
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
  autoStartProxy: boolean;
  overridePac: boolean;
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

export type ConnectionProtocol = "tcp" | "udp";

export interface ActiveConnection {
  id: number;
  host: string;
  port: number;
  policy: Policy;
  rule: string | null;
  startedAt: number;
  protocol: ConnectionProtocol;
}

export interface Bootstrap {
  config: AppConfig;
  rules: RulesSummary;
  status: ProxyStatus;
  stats: TrafficStats;
  active: ActiveConnection[];
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
  protocol: Protocol;
  transport: Transport;
  tls?: NodeTls | null;
  ws?: NodeWs | null;
  flow?: string | null;
  udp: boolean;
}

export interface SettingsInput {
  listenPort: number;
  allowLan: boolean;
  defaultPolicy: Policy;
  systemProxy: boolean;
  proxyEnv: boolean;
  launchAtStartup: boolean;
  autoStartProxy: boolean;
  overridePac: boolean;
  dnsServer: string;
  ipv6: boolean;
}

export interface TrafficPayload {
  stats: TrafficStats;
  running: boolean;
  active: ActiveConnection[];
}

export interface AddNodeResult {
  node: Node;
  warnings: string[];
}