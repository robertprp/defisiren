export interface Evidence {
  id: string;
  kind: string;
  description: string;
  transaction?: string | null;
  data: unknown;
}

export interface Finding {
  incident_id: string;
  event_id: string;
  revision: number;
  cursor: number;
  chain_id: number;
  observed_at: string;
  finding_type: string;
  severity: string;
  confidence: number;
  source: string;
  finality: string;
  reviewed: boolean;
  retracted: boolean;
  enrichment_complete: boolean;
  addresses: string[];
  suspected_victims: string[];
  observed_victims: string[];
  evidence: Evidence[];
  position: { block_number?: number | null; block_hash?: string | null };
  simulation: { status: string; fidelity: string; unresolved: string[]; assumptions: string[] };
}

export interface Capability {
  chain_id: number;
  family: string;
  pending_visibility: string;
  preconfirmation_semantics: string;
  traces: string[];
  historical_state: string;
  finality: string;
  verified_at: string | null;
  degraded: string[];
}

export interface Health {
  ready: boolean;
  database: boolean;
  chains_without_recent_probe: number[];
}

export interface RevisionPage {
  incident_id: string;
  revisions: Finding[];
  next_after: number;
  page_full: boolean;
}

export interface Profile {
  chain_id: number;
  address: string;
  first_observed: string;
  last_activity: string;
  history_complete: boolean;
  watch_until: string;
  code_status: string;
  freshness_confidence: number;
  deployments: string[];
  funding: { id: string; from: string; amount: string; asset: string | null; hop: number; transaction: string }[];
}

export const CHAINS: Record<number, { name: string; short: string; color: string }> = {
  1: { name: "Ethereum", short: "ETH", color: "#6b79b8" },
  8453: { name: "Base", short: "BASE", color: "#3866f3" },
  56: { name: "BNB Chain", short: "BNB", color: "#b68c12" },
  42161: { name: "Arbitrum", short: "ARB", color: "#3498b6" },
};

export const chainName = (id: number) => CHAINS[id]?.name ?? `Chain ${id}`;
export const humanize = (text: string) => text.replaceAll("_", " ");
export const shorten = (text: string) => text.length > 17 ? `${text.slice(0, 8)}…${text.slice(-6)}` : text;
export const isAlert = (f: Finding) => !f.retracted && ["high", "critical"].includes(f.severity);
export const dateTime = (text: string) => new Date(text).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
