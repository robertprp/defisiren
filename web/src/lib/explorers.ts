export const EXPLORERS: Record<number, { name: string; origin: string }> = {
  1: { name: "Etherscan", origin: "https://etherscan.io" },
  8453: { name: "Basescan", origin: "https://basescan.org" },
  56: { name: "BscScan", origin: "https://bscscan.com" },
  42161: { name: "Arbiscan", origin: "https://arbiscan.io" },
};

export function explorerUrl(chain: number, kind: "address" | "tx" | "block", value: string | number) {
  const explorer = EXPLORERS[chain];
  if (!explorer) return null;
  const text = String(value);
  const valid = kind === "address" ? /^0x[0-9a-fA-F]{40}$/.test(text)
    : kind === "tx" ? /^0x[0-9a-fA-F]{64}$/.test(text)
    : /^(0|[1-9][0-9]*)$/.test(text);
  return valid ? `${explorer.origin}/${kind}/${text}` : null;
}
