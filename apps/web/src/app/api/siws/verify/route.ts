import { jwtCannotSpend, queryClaims, verifySiws } from "@cpm/sdk";
import { SignJWT } from "jose";
import { NextResponse } from "next/server";
import { takeNonce } from "@/lib/siws-store";

function secret() {
  return new TextEncoder().encode(process.env.SIWS_SECRET ?? "dev-siws-not-for-prod");
}

export async function POST(req: Request) {
  const { address, message, signature } = (await req.json()) as {
    address?: string;
    message?: string;
    signature?: string;
  };
  if (!address || !message || !signature) {
    return NextResponse.json({ error: "missing fields" }, { status: 400 });
  }
  const nonce = /Nonce: (\w+)/.exec(message)?.[1];
  if (!nonce || !takeNonce(address, nonce)) {
    return NextResponse.json({ error: "bad nonce" }, { status: 400 });
  }
  const raw = Buffer.from(signature, "base64");
  const bs58 = (await import("bs58")).default;
  if (!verifySiws(message, address, bs58.encode(raw))) {
    return NextResponse.json({ error: "bad signature" }, { status: 401 });
  }
  const claims = queryClaims(address);
  if (!jwtCannotSpend(claims)) {
    return NextResponse.json({ error: "claims not query-only" }, { status: 500 });
  }
  const token = await new SignJWT({ purpose: claims.purpose })
    .setProtectedHeader({ alg: "HS256" })
    .setSubject(address)
    .setExpirationTime(claims.exp)
    .sign(secret());
  const res = NextResponse.json({ ok: true, purpose: "query" });
  res.cookies.set("cpm_jwt", token, { httpOnly: true, sameSite: "lax", path: "/", maxAge: 3600 });
  return res;
}
