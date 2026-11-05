"use client";

import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Copy, Check, ArrowUpRight, FileJson, Layers3 } from "lucide-react";
import { useState } from "react";
import { ExplorerLink } from "./explorer-link";
import { categoryOf, CATEGORY_LABELS, explainFinding } from "@/lib/triage";
import { monitor } from "@/lib/api";
import { demoRevisions } from "@/lib/demo";
import { dateTime, humanize, type Finding, type RevisionPage } from "@/lib/types";
import { ChainLabel, ErrorBox, Loading, Modal, Severity } from "./ui";

export function CopyValue({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState(false);
  return <button className="copy-value" title={value} onClick={async () => {
    try { await navigator.clipboard.writeText(value); setCopied(true); setError(false); }
    catch { setError(true); }
  }}><code>{value}</code>{copied ? <Check size={14} /> : <Copy size={14} />}<span className="sr-only">{error ? "Copy failed; select the address manually" : copied ? "Copied" : "Copy"}</span></button>;
}

export default function IncidentDetail({ finding, token, demo, onClose }: { finding: Finding; token: string; demo: boolean; onClose: () => void }) {
  const current = useQuery({ queryKey: ["incident", finding.incident_id], queryFn: ({ signal }) => demo ? Promise.resolve(finding) : monitor<Finding>(`v1/incidents/${finding.incident_id}`, token, signal), refetchInterval: demo ? false : 15_000 });
  const history = useInfiniteQuery({ queryKey: ["revisions", finding.incident_id], initialPageParam: 0,
    queryFn: ({ pageParam, signal }) => demo ? Promise.resolve(demoRevisions(finding)) : monitor<RevisionPage>(`v1/incidents/${finding.incident_id}/revisions?after=${pageParam}&limit=20`, token, signal),
    getNextPageParam: (page) => page.page_full ? page.next_after : undefined,
  });
  const item = current.data ?? finding;
  const explanation = explainFinding(item);
  return <Modal title="Incident investigation" drawer onClose={onClose}>
    <div className="detail-tags"><Severity value={item.severity} /><ChainLabel id={item.chain_id} /><span className="badge neutral">{item.finality}</span></div>
    <h3 className="incident-title">{explanation.title}</h3>
    <div className={`classification-note category-${categoryOf(item)}`}><strong>{CATEGORY_LABELS[categoryOf(item)]} · {explanation.basis}</strong><p>{explanation.explanation}</p></div>
    <p className="caption">Source: {item.source} · Detector: {item.finding_type}</p>
    <p className="muted">Observed {dateTime(item.observed_at)} · revision {item.revision}</p>
    {item.retracted && <div className="notice warning">This incident was retracted. Its earlier evidence is preserved below.</div>}
    {demo && <div className="notice">Synthetic demonstration. No real transaction or victim is represented.</div>}
    {current.error && <ErrorBox error={current.error} retry={() => void current.refetch()} />}
    <div className="detail-facts">
      <div><span>Evidence confidence</span><strong>{(item.confidence / 100).toFixed(1)}%</strong></div>
      <div><span>Review status</span><strong>{item.reviewed ? "Reviewed" : "Unreviewed"}</strong></div>
      <div><span>Block number</span><strong>{item.position.block_number != null ? <ExplorerLink chain={item.chain_id} kind="block" value={item.position.block_number} demo={demo || categoryOf(item) === "demo"} /> : "Not included"}</strong></div>
      <div><span>Enrichment</span><strong>{item.enrichment_complete ? "Complete" : "Pending"}</strong></div>
    </div>
    <p className="caption">Confidence describes evidence quality, not the probability of theft.</p>
    <section className="detail-section"><h4><ArrowUpRight size={16} /> Addresses</h4>{item.addresses.length ? item.addresses.map((address) => <div key={address}><ExplorerLink chain={item.chain_id} kind="address" value={address} demo={demo || categoryOf(item) === "demo"} /><CopyValue value={address} /></div>) : <p className="muted">No addresses attached.</p>}</section>
    <section className="detail-section"><h4><FileJson size={16} /> Evidence <span className="count">{item.evidence.length}</span></h4>
      {item.evidence.map((evidence) => <details className="evidence-card" key={evidence.id}><summary>{humanize(evidence.kind)}</summary><p>{evidence.description}</p>{evidence.transaction && <><ExplorerLink chain={item.chain_id} kind="tx" value={evidence.transaction} demo={demo || categoryOf(item) === "demo"}>View transaction</ExplorerLink><CopyValue value={evidence.transaction} /></>}<pre>{JSON.stringify(evidence.data, null, 2)}</pre></details>)}
      {!item.evidence.length && <p className="muted">No evidence attached.</p>}
    </section>
    <section className="detail-section"><h4>Simulation</h4><div className="notice"><strong>{humanize(item.simulation.status)} · {item.simulation.fidelity}</strong>{item.simulation.unresolved.map((reason, i) => <p key={i}>{reason}</p>)}</div></section>
    <section className="detail-section"><div className="section-heading"><h4><Layers3 size={16} /> Revision history</h4><button className="text-button" onClick={() => void history.refetch()}>Refresh history</button></div>
      <p className="caption">Oldest first · includes corrections and retractions</p>
      {history.isPending && <Loading label="Loading revisions…" />}
      {history.error && <ErrorBox error={history.error} retry={() => void history.refetch()} />}
      <div className="timeline">{history.data?.pages.flatMap((page) => page.revisions).map((revision) => <div className="timeline-item" key={revision.event_id}><span className="timeline-dot" /><div><strong>Revision {revision.revision} <span className="badge neutral">{revision.retracted ? "retracted" : revision.finality}</span></strong><p>{dateTime(revision.observed_at)} · {humanize(revision.finding_type)}</p><details><summary>View revision JSON</summary><pre>{JSON.stringify(revision, null, 2)}</pre></details></div></div>)}</div>
      {history.hasNextPage && <button className="button full-width" disabled={history.isFetchingNextPage} onClick={() => void history.fetchNextPage()}>{history.isFetchingNextPage ? "Loading…" : "Load more revisions"}</button>}
    </section>
    <details className="raw-data"><summary>Current incident JSON</summary><pre>{JSON.stringify(item, null, 2)}</pre></details>
  </Modal>;
}
