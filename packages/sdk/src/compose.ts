import { PublicKey, TransactionInstruction } from "@solana/web3.js";

export type ComposeOut = {
  program_id: string;
  keys: { pubkey: string; is_signer: boolean; is_writable: boolean }[];
  data_b64: string;
};

function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function toIx(out: ComposeOut): TransactionInstruction {
  return new TransactionInstruction({
    programId: new PublicKey(out.program_id),
    keys: out.keys.map((k) => ({
      pubkey: new PublicKey(k.pubkey),
      isSigner: k.is_signer,
      isWritable: k.is_writable,
    })),
    data: Buffer.from(b64ToBytes(out.data_b64)),
  });
}

export async function compose(api: string, body: Record<string, unknown>): Promise<TransactionInstruction> {
  const r = await fetch(`${api}/v1/compose`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!r.ok) throw new Error(`compose ${r.status} ${await r.text()}`);
  return toIx((await r.json()) as ComposeOut);
}

/** CreateBoard metas: creator, market, grid, system_program. */
export function marketPubkeyFromCreateIx(ix: TransactionInstruction): string {
  const market = ix.keys[1]?.pubkey;
  if (!market) throw new Error("create instruction missing market PDA");
  return market.toBase58();
}
