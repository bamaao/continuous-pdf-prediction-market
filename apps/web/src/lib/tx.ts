import { ComputeBudgetProgram, Connection, PublicKey, Transaction, TransactionInstruction } from "@solana/web3.js";
import { GATEWAY } from "./env";

export async function sendSigned(
  connection: Connection,
  signTransaction: (tx: Transaction) => Promise<Transaction>,
  payer: PublicKey,
  ixs: TransactionInstruction[],
): Promise<string> {
  const bh = await connection.getLatestBlockhash("confirmed");
  const tx = new Transaction({
    feePayer: payer,
    recentBlockhash: bh.blockhash,
  });
  tx.add(
    ComputeBudgetProgram.requestHeapFrame({ bytes: 256 * 1024 }),
    ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
    ...ixs,
  );
  const signed = await signTransaction(tx);
  const sig = await connection.sendRawTransaction(signed.serialize(), { skipPreflight: true });
  const conf = await connection.confirmTransaction({ signature: sig, ...bh }, "confirmed");
  if (conf.value.err) {
    const parsed = await connection.getTransaction(sig, {
      commitment: "confirmed",
      maxSupportedTransactionVersion: 0,
    });
    const logs = parsed?.meta?.logMessages?.filter((l) => /failed|Error|error/i.test(l)).slice(-6) ?? [];
    throw new Error(logs.join(" · ") || JSON.stringify(conf.value.err));
  }
  return sig;
}

export async function submitGateway(
  connection: Connection,
  signTransaction: (tx: Transaction) => Promise<Transaction>,
  payer: PublicKey,
  owner: PublicKey,
  market: PublicKey,
  nonce: number,
  ixs: TransactionInstruction[],
): Promise<{ status: string; sig: string }> {
  const bh = await connection.getLatestBlockhash("confirmed");
  const tx = new Transaction({ feePayer: payer, recentBlockhash: bh.blockhash });
  tx.add(
    ComputeBudgetProgram.requestHeapFrame({ bytes: 256 * 1024 }),
    ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
    ...ixs,
  );
  const signed = await signTransaction(tx);
  const raw = signed.serialize();
  const tx_b64 = btoa(Array.from(raw, (b) => String.fromCharCode(b)).join(""));
  const r = await fetch(`${GATEWAY}/v1/submit`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      tx_b64,
      owner: owner.toBase58(),
      market: market.toBase58(),
      nonce,
    }),
  });
  const body = await r.json();
  if (!r.ok) throw new Error(body?.error ?? `gateway ${r.status}`);
  if (body.status === "confirmed") return body;
  for (let i = 0; i < 80; i++) {
    await new Promise((res) => setTimeout(res, 250));
    const rec = await fetch(
      `${GATEWAY}/v1/receipt?owner=${owner.toBase58()}&market=${market.toBase58()}&nonce=${nonce}`,
    );
    if (!rec.ok) continue;
    const j = await rec.json();
    if (j.status === "confirmed") return j;
    if (j.status === "failed") throw new Error(j.error ?? "fill failed");
  }
  throw new Error("pending timeout");
}

export function sharesToQ(shares: number): bigint {
  return BigInt(Math.trunc(shares)) << 64n;
}
