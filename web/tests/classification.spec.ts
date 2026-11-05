import { expect, test } from "@playwright/test";
import { demoFindings } from "../src/lib/demo";
import { categoryOf, explainFinding } from "../src/lib/triage";
import { explorerUrl } from "../src/lib/explorers";
import { poolFor } from "../src/lib/pools";

test("neutral funding and compatibility notices are not review candidates", () => {
  const base = { ...demoFindings(Date.now())[0], source: "canonical_rpc", severity: "informational", retracted: false };
  expect(categoryOf({ ...base, finding_type: "funding_watch" })).toBe("watch");
  expect(categoryOf({ ...base, finding_type: "unsupported_envelope" })).toBe("diagnostics");
  expect(categoryOf({ ...base, triage: { archived: true, reason: "Known envelope" } })).toBe("archived");
  expect(categoryOf({ ...base, severity: "medium" })).toBe("review");
  expect(categoryOf({ ...base, severity: "high", retracted: true })).toBe("retracted");
  expect(categoryOf({ ...base, source: "synthetic_demo", severity: "critical" })).toBe("demo");
});

test("pool identities are scoped by chain and intermediate wallets are not mislabeled", () => {
  const shared = "0x84443CFd09A48AF6eF360C6976C5392aC5023a1F";
  expect(poolFor(56, shared)?.symbol).toBe("BNB");
  expect(poolFor(42161, shared)?.symbol).toBe("ETH");
  expect(poolFor(8453, shared)).toBeUndefined();
  const base = { ...demoFindings(Date.now())[0], finding_type: "funding_watch", chain_id: 56 };
  const direct = { ...base, evidence: [{ id: "direct", kind: "withdrawal", description: "", data: { from: shared } }] };
  expect(explainFinding(direct).title).toBe("0.1 BNB pool withdrawal");
  const indirect = { ...base, evidence: [{ id: "indirect", kind: "transfer_provenance", description: "", data: { from: `0x${"ab".repeat(20)}` } }] };
  expect(explainFinding(indirect).title).toBe("Transfer from a watched wallet");
  expect(explainFinding(indirect).pool).toBeUndefined();
});

test("explorer URLs are chain-aware and reject malformed values", () => {
  const address = `0x${"ab".repeat(20)}`;
  expect(explorerUrl(1, "address", address)).toBe(`https://etherscan.io/address/${address}`);
  expect(explorerUrl(8453, "address", address)).toBe(`https://basescan.org/address/${address}`);
  expect(explorerUrl(56, "address", address)).toBe(`https://bscscan.com/address/${address}`);
  expect(explorerUrl(42161, "block", 12)).toBe("https://arbiscan.io/block/12");
  expect(explorerUrl(999, "address", address)).toBeNull();
  expect(explorerUrl(1, "tx", address)).toBeNull();
  expect(explorerUrl(1, "address", "javascript:alert(1)")).toBeNull();
});
