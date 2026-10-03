import { jwtVerify } from "jose";
import { NextResponse } from "next/server";

const DEV_SECRET = "dev-siws-not-for-prod";

function secretBytes(): Uint8Array | null {
  const raw = process.env.SIWS_SECRET ?? "";
  const env = (process.env.CPM_ENV ?? process.env.NEXT_PUBLIC_CPM_ENV ?? "local").toLowerCase();
  if ((env === "production" || env === "prod" || env === "staging") && (!raw || raw === DEV_SECRET)) {
    return null;
  }
  return new TextEncoder().encode(raw || DEV_SECRET);
}

export async function GET(req: Request) {
  const secret = secretBytes();
  if (!secret) return NextResponse.json({ ok: false, error: "SIWS_SECRET required" });
  const cookie = req.headers.get("cookie") ?? "";
  const token = /(?:^|;\s*)cpm_jwt=([^;]+)/.exec(cookie)?.[1];
  if (!token) return NextResponse.json({ ok: false });
  try {
    const { payload } = await jwtVerify(token, secret);
    if (payload.purpose !== "query") return NextResponse.json({ ok: false });
    return NextResponse.json({ ok: true, sub: payload.sub, purpose: "query" });
  } catch {
    return NextResponse.json({ ok: false });
  }
}
