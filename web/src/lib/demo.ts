import type { Capability, Finding, Health, Profile, RevisionPage } from "./types";

export const DEMO_ADDRESS = "0x73b21c9840a7d5305f2ce189adc046f36890a112";
const chains = [1, 8453, 42161, 56];
const kinds = ["native_outflow", "funding_watch", "privilege_change", "watched_activity", "deployment_sequence", "bridge_transfer"];
const severities = ["high", "informational", "medium", "low", "critical", "informational"];

// Deliberately synthetic UI fixtures. These never enter the monitoring database.
export function demoFindings(now: number): Finding[] {
  return Array.from({ length: 48 }, (_, i) => {
    const kind = i % kinds.length;
    return {
      incident_id: `a1000000-0000-4000-8000-${String(i + 1).padStart(12, "0")}`,
      event_id: `b1000000-0000-4000-8000-${String(i + 1).padStart(12, "0")}`,
      revision: i % 3 + 1, cursor: i + 1, chain_id: chains[i % 4],
      observed_at: new Date(now - (i * 27 + 2) * 60_000).toISOString(),
      finding_type: kinds[kind], severity: severities[kind], confidence: 6200 + (i * 731) % 3600,
      source: "synthetic_demo", finality: i < 3 ? "included" : i < 10 ? "safe" : "finalized",
      reviewed: i % 4 === 0, retracted: i === 10 || i === 20, enrichment_complete: i > 2,
      addresses: [DEMO_ADDRESS], suspected_victims: [], observed_victims: [],
      position: { block_number: 21_480_126 - i * 2, block_hash: `0x${"a1".repeat(32)}` },
      evidence: [{ id: `demo-evidence-${i}`, kind: kinds[kind],
        description: "Synthetic evidence for exploring the dashboard. This is not a real incident or a determination of malicious activity.",
        transaction: `0x${(i + 1).toString(16).padStart(64, "0")}`,
        data: { synthetic: true, rule: kinds[kind], source: "dashboard fixture" } }],
      simulation: { status: "unsupported", fidelity: "none", unresolved: ["Demo data has no executable state snapshot."], assumptions: [] },
    };
  });
}

export function demoChains(now: number): Capability[] {
  return chains.map((chain_id) => ({ chain_id, family: chain_id === 1 ? "ethereum" : chain_id === 8453 ? "op" : chain_id === 56 ? "bsc" : "nitro",
    pending_visibility: "not enabled", preconfirmation_semantics: "not enabled", traces: ["callTracer", "prestateTracer"],
    historical_state: "available", finality: "native tags", verified_at: new Date(now - 25_000).toISOString(),
    degraded: chain_id === 42161 ? ["Sequencer feed unavailable; canonical RPC remains available."] : [],
  }));
}

export const demoHealth: Health = { ready: true, database: true, chains_without_recent_probe: [] };

export function demoRevisions(finding: Finding): RevisionPage {
  return { incident_id: finding.incident_id, next_after: finding.revision, page_full: false,
    revisions: Array.from({ length: finding.revision }, (_, i) => ({ ...finding, revision: i + 1,
      event_id: `${finding.event_id}-${i}`, finality: i === 0 ? "included" : finding.finality,
      observed_at: new Date(new Date(finding.observed_at).getTime() + i * 60_000).toISOString(),
    })) };
}

export function demoProfile(chain_id: number, now: number): Profile {
  return { chain_id, address: DEMO_ADDRESS, first_observed: new Date(now - 86400_000).toISOString(),
    last_activity: new Date(now - 120_000).toISOString(), watch_until: new Date(now + 604800_000).toISOString(),
    history_complete: false, code_status: "unknown", freshness_confidence: 0, deployments: [],
    funding: [{ id: "demo-funding", from: `0x${"b2".repeat(20)}`, amount: "1000000000000000000", asset: null, hop: 1, transaction: `0x${"c3".repeat(32)}` }] };
}
