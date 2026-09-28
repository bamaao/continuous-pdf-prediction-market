import { describe, expect, it } from "vitest";
import { Keypair, PublicKey, TransactionInstruction } from "@solana/web3.js";
import { marketPubkeyFromCreateIx } from "./compose";
import { DIRICHLET_SIMPLEX, DIRICHLET_TOP_N } from "./dirichlet";
import { listingIdHash, marketPda } from "./pda";

describe("listingIdHash Dirichlet layouts", () => {
  it("does not treat simplex / top-n as atoms for the same topic", () => {
    const topic = "election";
    const atoms = listingIdHash({ family: 3, topic, tag: "winner" });
    const simplex = listingIdHash({ family: 3, topic, tag: "winner", layout: DIRICHLET_SIMPLEX, bins: 10 });
    const topn = listingIdHash({ family: 3, topic, tag: "winner", layout: DIRICHLET_TOP_N, topN: 2 });
    expect(simplex.toString("hex")).not.toBe(atoms.toString("hex"));
    expect(topn.toString("hex")).not.toBe(atoms.toString("hex"));
    expect(marketPda(simplex).toBase58()).not.toBe(marketPda(atoms).toBase58());
  });

  it("ignores tag and unused Dirichlet fields", () => {
    const a = listingIdHash({ family: 3, topic: "election", tag: "a", layout: DIRICHLET_SIMPLEX, bins: 31, topN: 9 });
    const b = listingIdHash({ family: 3, topic: "election", tag: "b", layout: DIRICHLET_SIMPLEX, bins: 31 });
    expect(a.toString("hex")).toBe(b.toString("hex"));
  });
});

describe("marketPubkeyFromCreateIx", () => {
  it("reads CreateBoard market (keys[1])", () => {
    const creator = Keypair.generate().publicKey;
    const market = Keypair.generate().publicKey;
    const grid = Keypair.generate().publicKey;
    const ix = new TransactionInstruction({
      programId: Keypair.generate().publicKey,
      keys: [
        { pubkey: creator, isSigner: true, isWritable: true },
        { pubkey: market, isSigner: false, isWritable: true },
        { pubkey: grid, isSigner: false, isWritable: true },
        { pubkey: new PublicKey("11111111111111111111111111111111"), isSigner: false, isWritable: false },
      ],
      data: Buffer.alloc(0),
    });
    expect(marketPubkeyFromCreateIx(ix)).toBe(market.toBase58());
  });
});
