import type { Finding } from "./types";
import { poolFor } from "./pools";

export const CATEGORIES = ["review", "watch", "diagnostics", "retracted", "archived", "demo"] as const;
export type Category = typeof CATEGORIES[number];
export const CATEGORY_LABELS: Record<Category, string> = {
  review: "Needs review", watch: "Watch activity", diagnostics: "Diagnostics",
  retracted: "Retracted", archived: "Archived noise", demo: "Synthetic / replay",
};
export const CATEGORY_DESCRIPTIONS: Record<Category, string> = {
  review: "Active medium, high, or critical findings. These are review candidates, not confirmed exploits.",
  watch: "Neutral funding and wallet observations. Privacy use or deploying a contract does not establish malicious activity.",
  diagnostics: "Monitor coverage and protocol compatibility notices, not security alerts.",
  retracted: "Findings superseded by reorganizations or later evidence. Retained for investigation.",
  archived: "Known transaction formats incorrectly flagged by the old envelope rule. Evidence and revisions are preserved.",
  demo: "Synthetic demonstration or offline replay provenance. Not authenticated live monitoring data.",
};

export function categoryOf(f: Finding): Category {
  if (f.category && CATEGORIES.includes(f.category)) return f.category;
  if (f.triage?.archived) return "archived";
  if (["synthetic_demo", "offline_replay"].includes(f.source)) return "demo";
  if (f.retracted) return "retracted";
  if (["unsupported_envelope", "pending_alternative", "preconfirmation_correction", "pending_expired"].includes(f.finding_type)) return "diagnostics";
  return ["medium", "high", "critical"].includes(f.severity) ? "review" : "watch";
}

export function explainFinding(f: Finding) {
  const evidence = f.evidence[0];
  const data = (evidence?.data && typeof evidence.data === "object" ? evidence.data : {}) as Record<string, unknown>;
  if (f.finding_type === "funding_watch") {
    const from = typeof data.from === "string" ? data.from : undefined;
    const pool = poolFor(f.chain_id, from);
    if (evidence?.kind === "withdrawal") return {
      title: pool ? `${pool.denomination_display} ${pool.symbol} pool withdrawal` : "Registered pool withdrawal",
      explanation: pool ? `Withdrawal from ${pool.label}. This verifies funding provenance, not harmful intent.` : "A withdrawal event was recorded. Pool identity requires a matching chain-scoped registry entry.",
      basis: "Direct withdrawal", pool,
    };
    return { title: "Transfer from a watched wallet", explanation: "Funding provenance propagated from an intermediary. The immediate sender is not necessarily a Tornado Cash pool, and the transfer is not proof of abuse.", basis: "Indirect funding trail", pool };
  }
  if (f.finding_type === "suspicious_deployment" && data.known_exact_fingerprint === false) return {
    title: "Deployment after watched funding", explanation: "This wallet deployed a contract after observed funding. No exact malicious fingerprint was matched; funding alone does not establish harmful intent.", basis: "Funding + deployment",
  };
  if (f.finding_type === "unsupported_envelope") return {
    title: f.triage?.archived ? "Recognized protocol transaction" : "Transaction format diagnostic",
    explanation: f.triage?.reason ?? `Envelope type ${data.type ?? "unknown"} was retained for compatibility review. This is a monitor capability notice, not evidence of exploitation.`, basis: `Envelope type ${data.type ?? "unknown"}`,
  };
  return { title: f.finding_type.replaceAll("_", " "), explanation: evidence?.description ?? "Inspect the supporting evidence before drawing conclusions.", basis: evidence?.kind.replaceAll("_", " ") ?? "No attached evidence" };
}
