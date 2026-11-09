import type { NextConfig } from "next";
import path from "node:path";

// The pool registry is shared with Rust in ../config, outside the web package.
const repositoryRoot = path.resolve(__dirname, "..");

const config: NextConfig = {
  turbopack: { root: repositoryRoot },
  outputFileTracingRoot: repositoryRoot,
  poweredByHeader: false,
  async headers() {
    return [{ source: "/:path*", headers: [
      { key: "X-Content-Type-Options", value: "nosniff" },
      { key: "Referrer-Policy", value: "no-referrer" },
      { key: "X-Frame-Options", value: "DENY" },
    ] }];
  },
};
export default config;
