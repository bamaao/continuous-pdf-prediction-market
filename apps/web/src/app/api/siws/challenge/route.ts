import { buildSiwsMessage } from "@cpm/sdk";
import { NextResponse } from "next/server";
import { CHAIN_ID } from "@/lib/env";
import { putNonce } from "@/lib/siws-store";

export async function POST(req: Request) {
  const { address, uri } = (await req.json()) as { address?: string; uri?: string };
  if (!address || !uri) return NextResponse.json({ error: "address and uri" }, { status: 400 });
  const nonce = crypto.randomUUID().replace(/-/g, "").slice(0, 16);
  putNonce(address, nonce);
  const host = new URL(uri).host;
  const message = buildSiwsMessage({
    domain: host,
    address,
    uri,
    nonce,
    issuedAt: new Date().toISOString(),
    chainId: CHAIN_ID,
  });
  return NextResponse.json({ nonce, message });
}
