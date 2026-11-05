"use client";

import { useEffect, useMemo, useState } from "react";
import dynamic from "next/dynamic";
import { QueryClient, QueryClientProvider, useQuery, useQueryClient } from "@tanstack/react-query";
import { MotionConfig, motion } from "motion/react";
import { Activity, ArrowDown, ArrowDownToLine, ArrowRight, ArrowUpRight, BellRing, Check, ChevronLeft, ChevronRight, CircleHelp, FlaskConical, LayoutDashboard, ListFilter, LoaderCircle, Network, Plug, Radio, Radar, RefreshCw, Search, ShieldCheck, SlidersHorizontal, Unplug, Wallet } from "lucide-react";
import { ApiError, monitor } from "@/lib/api";
import { demoChains, demoFindings, demoHealth } from "@/lib/demo";
import { CHAINS, chainName, dateTime, humanize, isAlert, shorten, type Capability, type Finding, type Health } from "@/lib/types";
import { ChainIcon, ChainLabel, Empty, ErrorBox, Loading, Modal, Severity } from "./ui";
import IncidentDetail from "./incident-detail";
import AddressLookup from "./address-lookup";

const ActivityChart = dynamic(() => import("./activity-chart"), { ssr: false, loading: () => <Loading label="Loading activity chart…" /> });
type View = "overview" | "findings" | "chains" | "addresses";
const navigation = [{ id: "overview", label: "Overview", icon: LayoutDashboard }, { id: "findings", label: "Findings", icon: ListFilter }, { id: "chains", label: "Networks", icon: Network }, { id: "addresses", label: "Address explorer", icon: Wallet }] as const;

export default function Dashboard() {
  const [mounted, setMounted] = useState(false);
  const [session, setSession] = useState({ token: "", id: 0 });
  useEffect(() => setMounted(true), []);
  if (!mounted) return <main className="boot"><Radar size={32} /><span>Opening Chain Siren…</span></main>;
  return <Session key={session.id} token={session.token} onSession={(token) => setSession((previous) => ({ token, id: previous.id + 1 }))} />;
}

function Session({ token, onSession }: { token: string; onSession: (token: string) => void }) {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: {
    staleTime: 10_000, refetchOnWindowFocus: false,
    retry: (count, error) => !(error instanceof ApiError && [400, 401, 404, 410, 429].includes(error.status)) && count < 1,
  } } }));
  useEffect(() => () => { void client.cancelQueries(); client.clear(); }, [client]);
  return <QueryClientProvider client={client}><MotionConfig reducedMotion="user"><Workspace token={token} onSession={onSession} /></MotionConfig></QueryClientProvider>;
}

function Connect({ onClose, onConnect }: { onClose: () => void; onConnect: (key: string) => void }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  return <Modal title="Connect your monitor" onClose={onClose}><p className="muted">Use an API key from your running Chain Siren service. Your key stays in memory for this tab.</p>
    <form onSubmit={async (event) => { event.preventDefault(); setBusy(true); setError(null); try { await monitor("v1/chains", key.trim()); onConnect(key.trim()); } catch (e) { setError(e as Error); } finally { setBusy(false); } }}>
      <label className="field-label">API key<input type="password" autoComplete="off" value={key} onChange={(event) => setKey(event.target.value)} placeholder="cs_…" required maxLength={128} autoFocus /></label>
      {error && <ErrorBox error={error} />}
      <button type="submit" className="button primary full-width" disabled={busy || !key.trim()}>{busy ? <LoaderCircle size={16} className="spin" /> : <Plug size={16} />}{busy ? "Connecting…" : "Connect to live data"}</button>
    </form>
    <div className="setup-note"><span className="eyebrow">CREATE AN API KEY</span><code>cargo run --release -- --config config/local.toml api-key --name dashboard</code><p>Start the Rust service on port 8080. To use another address, set <code>SIREN_API_URL</code> in <code>web/.env.local</code>.</p></div>
  </Modal>;
}

function chainStatus(chain: Capability, health: Health | undefined, now: number) {
  if (!chain.verified_at || now - new Date(chain.verified_at).getTime() > 120_000 || health?.chains_without_recent_probe.includes(chain.chain_id)) return "Stale probe";
  return chain.degraded.length ? "Degraded" : "Reporting";
}

function Workspace({ token, onSession }: { token: string; onSession: (token: string) => void }) {
  const demo = !token;
  const client = useQueryClient();
  const [now, setNow] = useState(Date.now);
  const [demoNow] = useState(Date.now);
  const [view, setView] = useState<View>("overview");
  const [connect, setConnect] = useState(false);
  const [help, setHelp] = useState(false);
  const [selected, setSelected] = useState<Finding | null>(null);
  const [search, setSearch] = useState("");
  const [chain, setChain] = useState("all");
  const [severity, setSeverity] = useState("all");
  const [status, setStatus] = useState("all");
  const [hours, setHours] = useState(24);
  const [page, setPage] = useState(0);
  const [ascending, setAscending] = useState(false);
  const [paused, setPaused] = useState(false);
  useEffect(() => { const interval = setInterval(() => setNow(Date.now()), 15_000); return () => clearInterval(interval); }, []);
  const findings = useQuery({ queryKey: ["incidents"], queryFn: ({ signal }) => demo ? Promise.resolve({ incidents: demoFindings(demoNow) }) : monitor<{ incidents: Finding[] }>("v1/incidents?limit=256", token, signal), refetchInterval: demo || paused ? false : 15_000 });
  const chains = useQuery({ queryKey: ["chains"], queryFn: ({ signal }) => demo ? Promise.resolve({ chains: demoChains(demoNow) }) : monitor<{ chains: Capability[] }>("v1/chains", token, signal), refetchInterval: demo || paused ? false : 30_000 });
  const health = useQuery({ queryKey: ["health"], queryFn: ({ signal }) => demo ? Promise.resolve(demoHealth) : monitor<Health>("health/ready", token, signal), refetchInterval: demo || paused ? false : 30_000 });
  const all = findings.data?.incidents ?? [];
  const filtered = useMemo(() => all.filter((item) => (chain === "all" || String(item.chain_id) === chain)
    && (severity === "all" || item.severity === severity)
    && (status === "all" || (status === "reviewed" && item.reviewed) || (status === "active" && !item.retracted) || (status === "retracted" && item.retracted))
    && `${item.incident_id} ${item.finding_type} ${item.addresses.join(" ")} ${chainName(item.chain_id)}`.toLowerCase().includes(search.toLowerCase()))
    .sort((a, b) => (ascending ? 1 : -1) * (new Date(a.observed_at).getTime() - new Date(b.observed_at).getTime())), [all, chain, severity, status, search, ascending]);
  const pageSize = view === "overview" ? 6 : 15;
  const pages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const currentPage = Math.min(page, pages - 1);
  const rows = filtered.slice(currentPage * pageSize, (currentPage + 1) * pageSize);
  const addresses = new Set(all.flatMap((item) => item.addresses)).size;
  const heading = { overview: "Your onchain watchtower.", findings: "Follow every signal.", chains: "A view across networks.", addresses: "Trace the activity." }[view];
  const subheading = { overview: "A clear view of the signals that matter, across every monitored chain.", findings: "Inspect findings, follow the evidence, and understand what changed.", chains: "Provider capabilities, probe freshness, and coverage at a glance.", addresses: "Explore watched wallets and the evidence connecting them." }[view];
  const networkIds = [...new Set([...(chains.data?.chains.map((item) => item.chain_id) ?? []), ...(health.data?.chains_without_recent_probe ?? []), ...all.map((item) => item.chain_id)])];
  const refreshing = findings.isFetching || chains.isFetching || health.isFetching;
  function navigate(next: View) { setView(next); setPage(0); }
  function resetFilters() { setSearch(""); setChain("all"); setSeverity("all"); setStatus("all"); setPage(0); }
  function exportData() {
    const url = URL.createObjectURL(new Blob([JSON.stringify({ mode: demo ? "synthetic-demo" : "live", exported_at: new Date().toISOString(), scope: "filtered latest 256 incidents", incidents: filtered }, null, 2)], { type: "application/json" }));
    const link = document.createElement("a"); link.href = url; link.download = `chain-siren-${demo ? "demo-" : ""}findings.json`; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  return <div className="app-shell">
    <aside className="sidebar">
      <a className="brand" href="/" aria-label="Chain Siren home"><span className="brand-mark"><Radar size={26} strokeWidth={1.5} /></span><span>chain<span className="brand-light">siren</span><small>ONCHAIN INTELLIGENCE</small></span></a>
      <div className="workspace-label"><span className="workspace-avatar">S</span><span>Monitoring workspace<small>{demo ? "Demo environment" : "Connected monitor"}</small></span><span className="workspace-dot" /></div>
      <div className="nav-label">WORKSPACE</div>
      <nav aria-label="Main navigation">{navigation.map(({ id, label, icon: Icon }) => <button key={id} aria-label={label} className={`nav-item ${view === id ? "active" : ""}`} aria-current={view === id ? "page" : undefined} onClick={() => navigate(id)}><Icon size={18} strokeWidth={1.7} /><span>{label}</span>{id === "findings" && all.length > 0 && <span className="nav-count">{all.length}</span>}</button>)}</nav>
      <div className="sidebar-bottom"><div className="sidebar-insight"><span className="mini-radar"><Radio size={19} /></span><h3>Evidence over noise.</h3><p>Every signal has a story.<br />Look closer before taking action.</p><button onClick={() => setHelp(true)}>How to read findings <ArrowUpRight size={14} /></button></div>
        <button className="nav-item" onClick={() => setHelp(true)}><CircleHelp size={18} />Quick guide<ArrowUpRight size={14} /></button>
        <div className="sidebar-footer"><span className={`status-dot ${demo ? "demo-dot" : ""}`} />{demo ? "Preview mode" : "Monitor connected"}<span>v0.1</span></div>
      </div>
    </aside>
    <div className="main-shell">
      <header className="topbar"><div className="breadcrumb">Workspace <span>/</span><strong>{navigation.find((item) => item.id === view)?.label}</strong></div><div className="top-actions"><span className={`environment-pill ${demo ? "demo-pill" : ""}`}>{demo ? <FlaskConical size={13} /> : <span className="status-dot" />}{demo ? "Demo data" : "Live monitor"}</span><span className="top-divider" /><button className="button small-button" onClick={() => demo ? setConnect(true) : onSession("")}>{demo ? <Plug size={14} /> : <Unplug size={14} />}{demo ? "Connect live" : "Disconnect"}</button></div></header>
      <main className="main-content">
        <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.3 }}>
          <div className="page-heading"><div><div className="eyebrow heading-eyebrow"><span className="eyebrow-line" />{view === "overview" ? "MONITOR OVERVIEW" : view === "findings" ? "EVIDENCE FEED" : view === "chains" ? "NETWORK COVERAGE" : "ADDRESS INTELLIGENCE"}</div><h1>{heading}</h1><p>{subheading}</p></div><div className="heading-actions"><button className="icon-button bordered" aria-label="Refresh dashboard" disabled={refreshing} onClick={() => void client.invalidateQueries()}><RefreshCw size={16} className={refreshing ? "spin" : ""} /></button>{view !== "addresses" && <button className="button" onClick={exportData} disabled={!all.length}><ArrowDownToLine size={15} />Export findings</button>}</div></div>
          <div className={`context-banner ${demo ? "" : "live-banner"}`}><div>{demo ? <FlaskConical size={16} /> : <Radio size={16} />}<span>{demo ? <><strong>You’re exploring a demo.</strong> All signals and network statuses are synthetic.</> : <><strong>{paused ? "Auto-refresh paused." : "Live data mode."}</strong> Showing up to 256 most recently updated incidents.</>}</span></div>{demo ? <button onClick={() => setConnect(true)}>Connect your monitor <ArrowRight size={14} /></button> : <button onClick={() => setPaused((value) => !value)}>{paused ? "Resume updates" : "Pause updates"}</button>}</div>
          {findings.error && <ErrorBox error={findings.error} retry={() => void findings.refetch()} />}
          {health.error && <ErrorBox error={health.error} retry={() => void health.refetch()} />}
          {!demo && health.data && !health.data.ready && <div className="notice warning">Monitor not ready. {health.data.database ? "Database connected." : "Database unavailable."} {health.data.chains_without_recent_probe.length > 0 && `Missing recent probes: ${health.data.chains_without_recent_probe.map(chainName).join(", ")}.`}</div>}
          {(view === "overview" || view === "findings") && <>
            <div className="metrics-grid">{[
              { label: "Loaded incidents", value: findings.data ? all.length : "—", icon: Activity, note: "Latest 256 · all severities", accent: "green" },
              { label: "Priority signals", value: findings.data ? all.filter(isAlert).length : "—", icon: BellRing, note: "Active high & critical findings", accent: "orange" },
              { label: "Addresses in view", value: findings.data ? addresses : "—", icon: Wallet, note: "Unique addresses in loaded incidents", accent: "blue" },
              { label: "Reviewed findings", value: findings.data ? all.filter((item) => item.reviewed).length : "—", icon: ShieldCheck, note: "Operator-reviewed latest revisions", accent: "neutral" },
            ].map(({ label, value, icon: Icon, note, accent }, index) => <motion.div className="metric-card" key={label} initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: index * 0.035 }}><div className="metric-top"><span>{label}</span><span className={`metric-icon ${accent}`}><Icon size={17} strokeWidth={1.6} /></span></div><strong className="metric-number">{value}</strong><p>{note}</p></motion.div>)}</div>
            {view === "overview" && <div className="analytics-grid"><section className="panel activity-panel"><div className="section-heading"><div><h2>Signal activity <span className="subtle-dot" /></h2><p className="caption">Loaded incidents by observation hour</p></div><div className="segmented" aria-label="Chart time range">{[6, 24].map((value) => <button key={value} aria-pressed={hours === value} onClick={() => setHours(value)}>{value}h</button>)}</div></div><div className="chart-legend"><span><i className="green-legend" />All loaded incidents</span><span><i className="orange-legend" />Active high / critical</span></div><ActivityChart findings={all} now={demo ? demoNow : now} hours={hours} /><div className="chart-footnote"><Activity size={13} />Based on this snapshot, not total chain activity.</div></section>
              <section className="panel network-panel"><div className="section-heading"><div><h2>Network pulse</h2><p className="caption">Provider probe status</p></div><Network size={18} className="muted" /></div>{chains.isPending && <Loading />}{chains.error && <ErrorBox error={chains.error} retry={() => void chains.refetch()} />}
                <div className="network-list">{networkIds.map((id) => { const item = chains.data?.chains.find((value) => value.chain_id === id); const state = item ? chainStatus(item, health.data, demo ? demoNow : now) : "No probe"; return <button className="network-row" key={id} onClick={() => navigate("chains")}><ChainIcon id={id} /><span><strong>{chainName(id)}</strong><small>{item?.family ?? "Awaiting provider"}</small></span><span className={`network-state ${state !== "Reporting" ? "degraded" : ""}`}><i />{state}</span></button>; })}</div>
                {!chains.isPending && !networkIds.length && <p className="muted">No chain probes recorded yet.</p>}<button className="network-footer" onClick={() => navigate("chains")}>View network coverage <ArrowRight size={14} /></button>
              </section></div>}
            <section className="panel findings-panel"><div className="section-heading findings-heading"><div><h2>{view === "overview" ? "Recent findings" : "All loaded findings"}<span className="count">{filtered.length}</span></h2><p className="caption">Signals are observations, not proof of exploitation.</p></div>{view === "overview" && <button className="text-button" onClick={() => navigate("findings")}>View all findings <ArrowUpRight size={15} /></button>}</div>
              <div className="filter-bar"><label className="search-input"><Search size={16} /><input aria-label="Search findings" value={search} placeholder="Search type, address, or incident…" onChange={(event) => { setSearch(event.target.value); setPage(0); }} /></label><div className="filter-selects"><select aria-label="Filter by network" value={chain} onChange={(event) => { setChain(event.target.value); setPage(0); }}><option value="all">All networks</option>{networkIds.map((id) => <option key={id} value={id}>{chainName(id)}</option>)}</select><select aria-label="Filter by severity" value={severity} onChange={(event) => { setSeverity(event.target.value); setPage(0); }}><option value="all">All severities</option>{["critical", "high", "medium", "low", "informational"].map((value) => <option key={value}>{value}</option>)}</select><select aria-label="Filter by status" value={status} onChange={(event) => { setStatus(event.target.value); setPage(0); }}><option value="all">All statuses</option><option value="active">Active</option><option value="reviewed">Reviewed</option><option value="retracted">Retracted</option></select><button className="icon-button" aria-label="Reset filters" title="Reset filters" onClick={resetFilters}><SlidersHorizontal size={16} /></button></div></div>
              {findings.isPending ? <Loading /> : !rows.length ? <Empty title={all.length ? "No findings match your filters" : "Waiting for the first signal"} detail={all.length ? "Adjust your search or reset the filters to see more findings." : "Findings will appear as the Rust monitor observes relevant onchain activity."} /> : <div className="table-scroll"><table><thead><tr><th>Finding / address</th><th>Network</th><th>Severity</th><th>Status</th><th><button className="sort-button" onClick={() => { setAscending((value) => !value); setPage(0); }}>Observed <ArrowDown size={12} style={{ transform: ascending ? "rotate(180deg)" : "none" }} /></button></th><th><span className="sr-only">Open</span></th></tr></thead><tbody>{rows.map((item) => <tr key={item.incident_id}><td><button className="finding-title" onClick={() => setSelected(item)}>{humanize(item.finding_type)}</button><div className="finding-subtitle"><code>{item.addresses[0] ? shorten(item.addresses[0]) : shorten(item.incident_id)}</code><span>·</span><span>rev {item.revision}</span></div></td><td><ChainLabel id={item.chain_id} /></td><td><Severity value={item.severity} /></td><td><span className={`row-status ${item.retracted ? "retracted" : ""}`}>{item.retracted ? "Retracted" : item.reviewed ? <><Check size={12} />Reviewed</> : humanize(item.finality)}</span></td><td className="time-cell">{dateTime(item.observed_at)}</td><td><button className="icon-button" aria-label={`Inspect ${humanize(item.finding_type)} ${item.incident_id}`} onClick={() => setSelected(item)}><ArrowUpRight size={16} /></button></td></tr>)}</tbody></table></div>}
              <div className="table-footer"><span>{filtered.length ? `${currentPage * pageSize + 1}–${Math.min((currentPage + 1) * pageSize, filtered.length)} of ${filtered.length} findings` : "0 findings"}</span><div><button className="icon-button" disabled={currentPage === 0} onClick={() => setPage(currentPage - 1)} aria-label="Previous page"><ChevronLeft size={16} /></button><span>{currentPage + 1} / {pages}</span><button className="icon-button" disabled={currentPage + 1 >= pages} onClick={() => setPage(currentPage + 1)} aria-label="Next page"><ChevronRight size={16} /></button></div></div>
            </section>
          </>}
          {view === "chains" && <><div className="notice"><Radio size={17} /><span>“Reporting” means a recent capability probe, not complete trace or historical coverage. Review each capability below.</span></div>{chains.error && <ErrorBox error={chains.error} retry={() => void chains.refetch()} />}{chains.isPending && <Loading />}<div className="chain-grid">{networkIds.map((id) => { const item = chains.data?.chains.find((value) => value.chain_id === id); const state = item ? chainStatus(item, health.data, demo ? demoNow : now) : "No probe"; return <section className="panel chain-card" key={id}><div className="section-heading"><div className="chain-card-title"><ChainIcon id={id} /><div><h2>{chainName(id)}</h2><span className="caption">Chain ID {id}</span></div></div><span className={`badge ${state === "Reporting" ? "severity-low" : "severity-medium"}`}>{state}</span></div>{item ? <><dl><div><dt>Last verified</dt><dd>{item.verified_at ? dateTime(item.verified_at) : "Unverified"}</dd></div><div><dt>Pending visibility</dt><dd>{item.pending_visibility}</dd></div><div><dt>Preconfirmations</dt><dd>{item.preconfirmation_semantics}</dd></div><div><dt>Native traces</dt><dd>{item.traces.join(", ") || "Unavailable"}</dd></div><div><dt>Historical state</dt><dd>{item.historical_state}</dd></div><div><dt>Finality</dt><dd>{item.finality}</dd></div></dl>{item.degraded.map((reason, index) => <div className="notice warning" key={index}>{reason}</div>)}</> : <p className="muted">No capability report received for this network.</p>}</section>; })}</div>{!chains.isPending && !networkIds.length && <Empty title="No network reports yet" detail="Start the Rust monitor with your chain configuration to receive provider capability reports." />}</>}
          {view === "addresses" && <AddressLookup token={token} demo={demo} now={demo ? demoNow : now} />}
          <footer className="content-footer"><span><ShieldCheck size={13} />Observe. Verify. Understand.</span><span>{demo ? "Synthetic preview · no live data" : findings.dataUpdatedAt ? `Last refreshed ${new Date(findings.dataUpdatedAt).toLocaleTimeString()}` : "Waiting for monitor"}<span className="footer-dot">·</span>Chain Siren</span></footer>
        </motion.div>
      </main>
    </div>
    {connect && <Connect onClose={() => setConnect(false)} onConnect={onSession} />}
    {selected && <IncidentDetail finding={selected} token={token} demo={demo} onClose={() => setSelected(null)} />}
    {help && <Modal title="Read the signal, then the evidence." onClose={() => setHelp(false)}><div className="guide"><p>Chain Siren surfaces observations from configured EVM networks. A finding is a starting point for investigation.</p><h3>Severity and confidence</h3><p>Severity prioritizes review. Confidence measures evidence quality; neither establishes that an address is malicious.</p><h3>Revisions and retractions</h3><p>Open a finding to inspect its evidence and revision history. Reorganizations and later observations can revise or retract earlier findings.</p><h3>What this dashboard covers</h3><p>The overview loads the latest 256 updated incidents. Charts count that snapshot by observation time; they are not network-wide or all-time statistics. Live data refreshes every 15 seconds, with network probes every 30 seconds.</p><h3>Connect your service</h3><p>Run the Rust monitor, create an API key, and choose “Connect live.” The key is kept only in this tab’s memory. Reloading or disconnecting returns to demo mode.</p></div></Modal>}
  </div>;
}
