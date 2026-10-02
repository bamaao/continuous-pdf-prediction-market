import { beforeEach, describe, expect, it } from "vitest";
import { ingestNotifyEvents, notifyKeysOnly } from "./tickets";

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
    expect(hit?.title).toBe("close");
  });
});
