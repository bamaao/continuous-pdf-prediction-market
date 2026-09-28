import nacl from "tweetnacl";
import bs58 from "bs58";

export const SIWS_PURPOSE = "query" as const;

export type SiwsMessage = {
  domain: string;
  address: string;
  uri: string;
  nonce: string;
  issuedAt: string;
  chainId: string;
};

export function buildSiwsMessage(m: SiwsMessage): string {
  return [
    `${m.domain} wants you to sign in with your Solana account:`,
    m.address,
    "",
    "URI: " + m.uri,
    "Version: 1",
    "Chain ID: " + m.chainId,
    "Nonce: " + m.nonce,
    "Issued At: " + m.issuedAt,
    "",
    "This signature only binds a query session. It cannot buy_set, withdraw, or move USDC.",
  ].join("\n");
}

export function verifySiws(message: string, address: string, signatureB58: string): boolean {
  try {
    const sig = bs58.decode(signatureB58);
    const pub = bs58.decode(address);
    const msg = new TextEncoder().encode(message);
    if (sig.length !== 64 || pub.length !== 32) return false;
    if (!message.includes(address)) return false;
    if (!message.includes("cannot buy_set")) return false;
    return nacl.sign.detached.verify(msg, sig, pub);
  } catch {
    return false;
  }
}

export type QueryJwt = {
  sub: string;
  purpose: typeof SIWS_PURPOSE;
  exp: number;
};

export function queryClaims(address: string, ttlSec = 3600): QueryJwt {
  return {
    sub: address,
    purpose: SIWS_PURPOSE,
    exp: Math.floor(Date.now() / 1000) + ttlSec,
  };
}

export function jwtCannotSpend(claims: QueryJwt): boolean {
  return claims.purpose === SIWS_PURPOSE && !("secret" in claims) && !("keypair" in claims);
}
