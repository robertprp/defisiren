// One registry for Rust ingestion and UI labels. Addresses are always chain-scoped.
import registry from "../../../config/tornado.v1.json";
export const POOLS = registry;
export function poolFor(chain: number, address: string | null | undefined) {
  return address ? POOLS.find((pool) => pool.chain_id === chain && pool.address.toLowerCase() === address.toLowerCase()) : undefined;
}
