import { jwtVerify } from "jose";
import { NextResponse } from "next/server";

export async function GET(req: Request) {
  const cookie = req.headers.get("cookie") ?? "";
  const token = /(?:^|;\s*)cpm_jwt=([^;]+)/.exec(cookie)?.[1];
  if (!token) return NextResponse.json({ ok: false });
  try {
    const { payload } = await jwtVerify(token, new TextEncoder().encode(process.env.SIWS_SECRET ?? "dev-siws-not-for-prod"));
    if (payload.purpose !== "query") return NextResponse.json({ ok: false });
    return NextResponse.json({ ok: true, sub: payload.sub, purpose: "query" });
  } catch {
    return NextResponse.json({ ok: false });
  }
}
