import {
  compose,
  gridP0Len,
  gridPda,
  gridShardPda,
  gridZ,
  marketN,
  marketPubkeyFromCreateIx,
  saveListing,
  shardCount,
} from "@cpm/sdk";
import { PublicKey, type Connection, type Transaction } from "@solana/web3.js";
import { sendSigned } from "./tx";

export type BoardComposeSpec = Record<string, unknown>;

const DROP = new Set([
  "close_in",
  "tap_cap",
  "title",
  "tags",
  "event",
  "description",
  "blocked_regions",
  "source_locale",
  "i18n",
]);

export async function openBoardAsOwner(args: {
  api: string;
  connection: Connection;
  signTransaction: (tx: Transaction) => Promise<Transaction>;
  owner: PublicKey;
  spec: BoardComposeSpec;
  listing: {
    title: string;
    tags: string[];
    topic: string;
    tag: string;
    description: string;
    event: string;
    blocked_regions?: string[];
    source_locale?: string;
    i18n?: Record<string, { title?: string; event?: string; description?: string }>;
    image_id?: string;
  };
}): Promise<{ market: string; sig: string; created: boolean }> {
  const owner = args.owner.toBase58();
  const family = Number(args.spec.family ?? 0);
  const topic = String(args.spec.topic ?? args.listing.topic ?? "");
  const tag = String(args.spec.tag ?? args.listing.tag ?? "");
  if (!topic.trim() || !tag.trim()) {
    throw new Error("compose spec missing topic / tag");
  }
  const op = String(args.spec.op ?? "");
  if (!op) {
    throw new Error("compose spec missing op");
  }
  const now = Math.floor(Date.now() / 1000);
  let close_ts = Number(args.spec.close_ts ?? 0);
  if (!close_ts) {
    close_ts = now + Math.max(1, Number(args.spec.close_in ?? 86_400));
  }
  if (close_ts <= now) {
    throw new Error("close_ts is already past — reject this application or ask the applicant to resubmit");
  }
  const tapCap = Number(args.spec.tap_cap ?? 0);
  const rest: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(args.spec)) {
    if (!DROP.has(k)) rest[k] = v;
  }
  const layout = Number(args.spec.layout ?? 0);
  const topN = layout === 1 ? Number(args.spec.top_n ?? 0) : 0;
  const bins = layout === 2 ? Number(args.spec.bins ?? 0) : 0;
  const k = Number(args.spec.k ?? 0);
  const createIx = await compose(args.api, {
    ...rest,
    op,
    owner,
    family,
    topic,
    tag,
    close_ts,
    risk_lock_ts: Number(args.spec.risk_lock_ts ?? close_ts),
    c_m: 0,
    ...(family === 3
      ? {
          layout,
          top_n: topN,
          bins,
          ...(k > 0 ? { k } : {}),
        }
      : {}),
  });
  const market = marketPubkeyFromCreateIx(createIx);
  const existing = await args.connection.getAccountInfo(new PublicKey(market));
  let sig = "";
  let created = false;
  if (!existing) {
    sig = await sendSigned(args.connection, args.signTransaction, args.owner, [createIx]);
    created = true;
  }
  const nHint = Number(rest.n ?? args.spec.n ?? (family === 1 || family === 2 ? 256 : 0));
  sig = (await finishGrid(args, owner, market, nHint)) || sig;
  try {
    const fundSig = await sendSigned(args.connection, args.signTransaction, args.owner, [
      await compose(args.api, { op: "fund_cm", owner, market, amount: 0 }),
    ]);
    sig = sig || fundSig;
  } catch {
    /* board vault may already be open */
  }
  try {
    await sendSigned(args.connection, args.signTransaction, args.owner, [
      await compose(args.api, { op: "risk_open_book", owner, market }),
    ]);
  } catch {
    /* book may already exist */
  }
  if (tapCap > 0) {
    try {
      await sendSigned(args.connection, args.signTransaction, args.owner, [
        await compose(args.api, { op: "set_tap", owner, market, amount: tapCap }),
      ]);
    } catch {
      /* tap is optional */
    }
  }
  await saveListing(args.api, {
    market,
    title: args.listing.title,
    tags: args.listing.tags,
    category: args.listing.tags[0],
    topic,
    tag,
    description: args.listing.description,
    event: args.listing.event,
    blocked_regions: args.listing.blocked_regions ?? [],
    source_locale: args.listing.source_locale ?? "en",
    i18n: args.listing.i18n ?? {},
    image_id: args.listing.image_id ?? "",
  });
  return { market, sig, created };
}

async function finishGrid(
  args: {
    api: string;
    connection: Connection;
    signTransaction: (tx: Transaction) => Promise<Transaction>;
    owner: PublicKey;
  },
  owner: string,
  market: string,
  nHint: number,
): Promise<string> {
  const mpk = new PublicKey(market);
  const macc = await args.connection.getAccountInfo(mpk);
  if (!macc) return "";
  const n = marketN(macc.data) || nHint;
  if (n < 2) return "";
  const count = shardCount(n);
  const extra = Math.max(0, count - 1);
  let sig = "";
  const sendOp = async (body: Record<string, unknown>) => {
    sig = await sendSigned(args.connection, args.signTransaction, args.owner, [
      await compose(args.api, { owner, market, ...body }),
    ]);
  };
  for (let ix = 1; ix < count; ix++) {
    const shard = gridShardPda(mpk, ix);
    const acc = await args.connection.getAccountInfo(shard);
    if (!acc) {
      try {
        await sendOp({ op: "create_grid_shard", ix });
      } catch (e) {
        const msg = String(e);
        if (!/already in use|0x0/i.test(msg)) throw e;
      }
    }
    await sendOp({ op: "write_grid_shard", ix });
  }
  await sendOp({ op: "write_grid_mass" });
  if (extra > 16) {
    const extras = Array.from({ length: extra }, (_, i) => i + 1);
    for (let i = 0; i < extras.length; i += 16) {
      await sendOp({ op: "accum_seal", extras: extras.slice(i, i + 16) });
    }
    for (let i = 0; i < extras.length; i += 16) {
      await sendOp({ op: "apply_seal", extras: extras.slice(i, i + 16) });
    }
  } else {
    await sendOp({ op: "seal_grid", n });
  }
  const grid = await args.connection.getAccountInfo(gridPda(mpk));
  const local0 = Math.min(16, n);
  if (!grid || gridP0Len(grid.data) !== local0 || gridZ(grid.data) === 0n) {
    throw new Error(`grid not sealed (n=${n}, shard0 p0=${grid ? gridP0Len(grid.data) : 0})`);
  }
  return sig;
}
