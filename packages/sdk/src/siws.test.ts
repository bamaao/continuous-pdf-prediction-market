import { Keypair } from "@solana/web3.js";
import nacl from "tweetnacl";
import bs58 from "bs58";
import { describe, expect, it } from "vitest";
import { buildSiwsMessage, jwtCannotSpend, queryClaims, SIWS_PURPOSE, verifySiws } from "./siws";

describe("SIWS", () => {
  it("verifies a wallet signature and issues query-only claims", () => {
    const kp = Keypair.generate();
    const msg = buildSiwsMessage({
      domain: "localhost",
      address: kp.publicKey.toBase58(),
      uri: "http://localhost:3000",
      nonce: "abc12345",
      issuedAt: "2026-09-24T00:00:00.000Z",
      chainId: "localnet",
    });
    const sig = nacl.sign.detached(new TextEncoder().encode(msg), kp.secretKey);
    expect(verifySiws(msg, kp.publicKey.toBase58(), bs58.encode(sig))).toBe(true);
    const claims = queryClaims(kp.publicKey.toBase58());
    expect(claims.purpose).toBe(SIWS_PURPOSE);
    expect(jwtCannotSpend(claims)).toBe(true);
  });

  it("rejects a signature over a spend-shaped payload", () => {
    const kp = Keypair.generate();
    const forged = "buy_set now";
    const sig = nacl.sign.detached(new TextEncoder().encode(forged), kp.secretKey);
    expect(verifySiws(forged, kp.publicKey.toBase58(), bs58.encode(sig))).toBe(false);
  });
});
