import { ArrowUpRight } from "lucide-react";
import { explorerUrl, EXPLORERS } from "@/lib/explorers";
import { poolFor } from "@/lib/pools";
import { shorten } from "@/lib/types";

export function ExplorerLink({ chain, kind, value, demo = false, children }: { chain: number; kind: "address" | "tx" | "block"; value: string | number; demo?: boolean; children?: React.ReactNode }) {
  const href = demo ? null : explorerUrl(chain, kind, value);
  const pool = kind === "address" ? poolFor(chain, String(value)) : undefined;
  const label = children ?? pool?.label ?? (kind === "block" ? String(value) : shorten(String(value)));
  return href ? <a className="explorer-link" href={href} target="_blank" rel="noopener noreferrer" title={`${value} · ${EXPLORERS[chain].name}`}>{label}<ArrowUpRight size={12} /><span className="sr-only"> on {EXPLORERS[chain].name} (opens new tab)</span></a> : <span title={String(value)}>{label}</span>;
}
