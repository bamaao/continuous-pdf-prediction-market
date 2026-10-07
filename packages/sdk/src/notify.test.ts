import { beforeEach, describe, expect, it } from "vitest";
import {
  inboxTitle,
  ingestNotifyEvents,
  isInboxUnread,
  markAllInboxRead,
  markInboxRead,
  notifyKindLabel,
  notifyKeysOnly,
  pushInbox,
  unreadInboxCount,
} from "./tickets";

function memoryStorage(): Storage {
  const m = new Map<string, string>();
  return {
    get length() {
      return m.size;
    },
    clear: () => m.clear(),
    getItem: (k) => m.get(k) ?? null,
    key: (i) => [...m.keys()][i] ?? null,
    removeItem: (k) => {
      m.delete(k);
    },
    setItem: (k, v) => {
      m.set(k, v);
    },
  };
}

describe("notify payload", () => {
  beforeEach(() => {
    Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: memoryStorage() });
  });

  it("accepts market_id-only events", () => {
    expect(notifyKeysOnly({ ts: 1, kind: "commit", market: "MktAAA" })).toBe(true);
  });

  it("rejects extra identity fields", () => {
    expect(notifyKeysOnly({ ts: 1, kind: "commit", market: "MktAAA", owner: "Own" })).toBe(false);
  });

  it("ingests feed items keyed by market", () => {
    const rows = ingestNotifyEvents([{ ts: 9, kind: "close", market: "MktBBB" }]);
    const hit = rows.find((r) => r.id === "ntf-9-close-MktBBB");
    expect(hit?.market).toBe("MktBBB");
    expect(hit?.kind).toBe("close");
    expect(hit?.title).toBe(notifyKindLabel("close"));
    expect(inboxTitle({ title: "close" })).toContain("Trading closed");
    expect(inboxTitle({ title: "Book committed from the rollup to L1", kind: "commit" })).toContain("committed");
  });

  it("merges market identity onto an existing inbox row", async () => {
    const { loadInbox } = await import("./tickets");
    pushInbox({
      id: "ntf-9-close-MktBBB",
      kind: "close",
      title: notifyKindLabel("close"),
      market: "MktBBB",
      ts: 9,
    });
    pushInbox({
      id: "ntf-9-close-MktBBB",
      kind: "close",
      title: notifyKindLabel("close"),
      market: "MktBBB",
      market_title: "Arsenal vs Chelsea",
      market_tags: "football · epl",
      market_status: "Halted",
      ts: 9,
    });
    const hit = loadInbox().find((r) => r.id === "ntf-9-close-MktBBB");
    expect(hit?.market_title).toBe("Arsenal vs Chelsea");
    expect(hit?.market_tags).toBe("football · epl");
    expect(hit?.market_status).toBe("Halted");
    expect(isInboxUnread(hit!)).toBe(true);
  });

  it("tracks unread vs history without re-arming a read alert", () => {
    pushInbox({
      id: "a1",
      title: "Market settled — claim payout if you won",
      market: "M1",
      market_title: "US CPI YoY",
      ts: 1,
      read: false,
    });
    expect(unreadInboxCount()).toBe(1);
    markInboxRead("a1");
    expect(unreadInboxCount()).toBe(0);
    pushInbox({
      id: "a1",
      title: "Market settled — claim payout if you won",
      market: "M1",
      ts: 1,
      read: false,
    });
    expect(unreadInboxCount()).toBe(0);
    pushInbox({
      id: "a2",
      title: notifyKindLabel("close"),
      kind: "close",
      market: "M2",
      ts: 2,
    });
    expect(unreadInboxCount()).toBe(1);
    markAllInboxRead();
    expect(unreadInboxCount()).toBe(0);
  });
});
