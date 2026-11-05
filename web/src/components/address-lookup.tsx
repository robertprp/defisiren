"use client";

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Search, Wallet, ArrowRight } from "lucide-react";
import { monitor, ApiError } from "@/lib/api";
import { CHAINS, dateTime, type Profile } from "@/lib/types";
import { DEMO_ADDRESS, demoProfile } from "@/lib/demo";
import { ChainLabel, Empty, ErrorBox, Loading } from "./ui";
import { CopyValue } from "./incident-detail";

export default function AddressLookup({ token, demo, now }: { token: string; demo: boolean; now: number }) {
  const [address, setAddress] = useState("");
  const [chain, setChain] = useState("1");
  const [search, setSearch] = useState<{ address: string; chain: string } | null>(null);
  const [validation, setValidation] = useState("");
  const query = useQuery({ queryKey: ["address", search], enabled: !!search,
    queryFn: ({ signal }) => {
      if (demo) {
        if (search!.address.toLowerCase() !== DEMO_ADDRESS.toLowerCase()) throw new ApiError(404, "This address is not in the synthetic dataset. Try the sample address.");
        return Promise.resolve(demoProfile(Number(search!.chain), now));
      }
      return monitor<Profile>(`v1/addresses/${search!.chain}/${search!.address}`, token, signal);
    }, retry: false,
  });
  return <>
    <section className="panel lookup-panel"><div className="section-heading"><div><span className="eyebrow">FOLLOW THE EVIDENCE</span><h2>Look up an address</h2><p className="muted">Inspect a wallet’s watch profile, funding evidence, and deployments.</p></div><Wallet size={30} strokeWidth={1.2} /></div>
      <form className="lookup-form" onSubmit={(event) => { event.preventDefault(); const value = address.trim(); if (!/^0x[0-9a-fA-F]{40}$/.test(value)) { setValidation("Enter a valid 42-character EVM address starting with 0x."); return; } setValidation(""); setSearch({ address: value, chain }); }}>
        <label><span>Network</span><select value={chain} onChange={(event) => setChain(event.target.value)}>{Object.entries(CHAINS).map(([id, info]) => <option key={id} value={id}>{info.name}</option>)}</select></label>
        <label className="address-input"><span>Wallet address</span><input value={address} placeholder="0x…" autoComplete="off" spellCheck={false} onChange={(event) => setAddress(event.target.value)} /></label>
        <button className="button primary" type="submit"><Search size={16} />Look up</button>
      </form>
      {validation && <p role="alert" className="validation">{validation}</p>}
      {demo && <button className="text-button sample-address" onClick={() => { setAddress(DEMO_ADDRESS); setSearch({ address: DEMO_ADDRESS, chain }); setValidation(""); }}>Try the sample address <ArrowRight size={14} /></button>}
    </section>
    {!search && <Empty title="Every address has a trail" detail="Search for a watched address to explore the evidence behind its profile." />}
    {search && query.isPending && <Loading label="Looking up watch profile…" />}
    {query.error && <ErrorBox error={query.error} retry={() => void query.refetch()} />}
    {query.data && <section className="panel profile-panel"><div className="section-heading"><h2>Watch profile</h2><ChainLabel id={query.data.chain_id} /></div><CopyValue value={query.data.address} />
      <div className="detail-facts"><div><span>First observed</span><strong>{dateTime(query.data.first_observed)}</strong></div><div><span>Last activity</span><strong>{dateTime(query.data.last_activity)}</strong></div><div><span>Watch expires</span><strong>{dateTime(query.data.watch_until)}</strong></div><div><span>Historical coverage</span><strong>{query.data.history_complete ? "Complete" : "Partial / unknown"}</strong></div></div>
      <div className="notice">A watch indicates observed funding or operator enrollment. It does not establish malicious activity.</div>
      <h3>Funding evidence <span className="count">{query.data.funding.length}</span></h3>
      {query.data.funding.length === 0 && <p className="muted">No funding edges recorded.</p>}
      {query.data.funding.map((edge) => <div className="funding-row" key={edge.id}><span className="eyebrow">HOP {edge.hop}</span><CopyValue value={edge.from} /><span><strong>{edge.amount}</strong> base units · {edge.asset ?? "native asset"}</span></div>)}
      <h3>Deployments <span className="count">{query.data.deployments.length}</span></h3>
      {query.data.deployments.length ? query.data.deployments.map((item) => <CopyValue key={item} value={item} />) : <p className="muted">No deployments recorded.</p>}
    </section>}
  </>;
}
