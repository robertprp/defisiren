import { NextRequest, NextResponse } from "next/server";

export const dynamic = "force-dynamic";
const noCache = { "Cache-Control": "no-store, max-age=0" };
const errors: Record<number, string> = {
  400: "Invalid request. Check the address, chain, or pagination values.",
  401: "API key rejected or expired. Reconnect with a valid key.",
  404: "No record found in this monitor.",
  410: "This cursor has expired. Reload the current findings to start again.",
  429: "The monitor's request limit was reached. Wait a minute, then retry.",
  503: "The monitor's database is unavailable. Check the Rust service.",
};

export async function GET(request: NextRequest, context: { params: Promise<{ path: string[] }> }) {
  const { path } = await context.params;
  const route = path.join("/");
  const uuid = "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}";
  const allowed = route === "v1/incidents/summary" || /^(v1\/(incidents|chains|findings|reviewed)|health\/ready)$/.test(route)
    || new RegExp(`^v1/incidents/${uuid}(/revisions)?$`).test(route)
    || /^v1\/addresses\/[1-9][0-9]{0,17}\/0x[0-9a-fA-F]{40}$/.test(route);
  if (!allowed) return NextResponse.json({ error: "Unknown dashboard endpoint." }, { status: 404, headers: noCache });
  const authorization = request.headers.get("authorization");
  if (!authorization || !/^Bearer [^\s]{1,128}$/.test(authorization)) {
    return NextResponse.json({ error: errors[401] }, { status: 401, headers: noCache });
  }
  for (const key of request.nextUrl.searchParams.keys()) {
    if (!["after", "limit", "chain_id", "severity", "category"].includes(key)) {
      return NextResponse.json({ error: errors[400] }, { status: 400, headers: noCache });
    }
  }
  try {
    const base = new URL(process.env.SIREN_API_URL ?? "http://127.0.0.1:8080");
    if (!["http:", "https:"].includes(base.protocol) || base.username || base.password) throw new Error("Invalid origin");
    const url = new URL(`/${route}`, base.origin);
    url.search = request.nextUrl.search;
    const upstream = await fetch(url, {
      headers: { Authorization: authorization, Accept: "application/json" },
      cache: "no-store",
      redirect: "error",
      signal: AbortSignal.timeout(10_000),
    });
    if (!upstream.ok && !(route === "health/ready" && upstream.status === 503)) {
      await upstream.body?.cancel();
      return NextResponse.json({ error: errors[upstream.status] ?? "The Rust API returned an unexpected response." }, { status: upstream.status, headers: noCache });
    }
    const reader = upstream.body?.getReader();
    if (!reader) throw new Error("Empty response");
    const chunks: Uint8Array[] = [];
    let bytes = 0;
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
      if (bytes > 8 * 1024 * 1024) { await reader.cancel(); throw new Error("Response too large"); }
      chunks.push(value);
    }
    const body = Buffer.concat(chunks);
    return NextResponse.json(JSON.parse(body.toString("utf8")), { status: upstream.status, headers: noCache });
  } catch {
    // Never reflect provider URLs, credentials, or raw upstream errors to the browser.
    return NextResponse.json({ error: "Cannot reach the monitor. Start the Rust service and check SIREN_API_URL." }, { status: 502, headers: noCache });
  }
}
