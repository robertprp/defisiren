import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Chain Siren · Monitor",
  description: "Evidence-first monitoring across Ethereum, Base, BNB Chain, and Arbitrum.",
  icons: { icon: "/icon.svg" },
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return <html lang="en"><body>{children}</body></html>;
}
