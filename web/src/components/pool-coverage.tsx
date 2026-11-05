import { POOLS } from "@/lib/pools";
import { chainName } from "@/lib/types";
import { ExplorerLink } from "./explorer-link";

export default function PoolCoverage() {
  return <section className="panel pool-coverage"><div className="section-heading"><div><h2>Tornado Cash registry</h2><p className="caption">Chain-scoped identities from pinned official deployment records. Runtime probes check code and denomination before ingestion.</p></div></div>
    <div className="notice warning">Base: no verified official Classic deployment found in the referenced registry. No Base pool addresses are assumed. Arbitrum’s 3,300,000 boundary is an upstream scan start, not a verified creation block.</div>
    <div className="table-scroll"><table><thead><tr><th>Network</th><th>Pool</th><th>Contract</th><th>Provenance</th></tr></thead><tbody>{POOLS.map((pool) => <tr key={`${pool.chain_id}:${pool.address}`}><td>{chainName(pool.chain_id)}</td><td>{pool.denomination_display} {pool.symbol}</td><td><ExplorerLink chain={pool.chain_id} kind="address" value={pool.address}>{pool.address.slice(0, 8)}…{pool.address.slice(-6)}</ExplorerLink></td><td><a className="explorer-link" href={pool.provenance_url} target="_blank" rel="noopener noreferrer">Official registry ↗</a></td></tr>)}</tbody></table></div>
    <p className="caption">BNB Chain pools hold BNB, not ETH. Addresses can repeat across chains; a label is valid only for its chain. Source registration does not certify historical coverage or imply malicious use.</p>
  </section>;
}
