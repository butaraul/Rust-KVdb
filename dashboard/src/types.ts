export interface TopKey {
  key: string;
  count: number;
}

export interface MetricsPayload {
  total_ops: number;
  ops_per_sec: number;
  memory_bytes: number;
  memory_live_bytes: number;
  key_count: number;
  top_keys: TopKey[];
}

export interface OpsPoint {
  t: number;
  opsPerSec: number;
}
