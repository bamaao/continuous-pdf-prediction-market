import { describe, expect, it } from "vitest";
import {
  isAndroidUserAgent,
  isIosUserAgent,
  needsInWalletBrowse,
  phantomBrowseUrl,
  readInjectedWallet,
  solflareBrowseUrl,
} from "./wallet-shell";

describe("wallet shell", () => {
  it("treats iPhone as iOS", () => {
    expect(isIosUserAgent("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)")).toBe(true);
    expect(isIosUserAgent("Mozilla/5.0 (Windows NT 10.0)")).toBe(false);
  });

  it("treats Android phones as Android", () => {
    expect(isAndroidUserAgent("Mozilla/5.0 (Linux; Android 14; Pixel 8)")).toBe(true);
  });

  it("needs in-wallet browse on iOS without an injected provider", () => {
    expect(
      needsInWalletBrowse({
        userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)",
        injected: false,
      }),
    ).toBe(true);
  });

  it("does not need browse when Phantom is injected", () => {
    expect(
      needsInWalletBrowse({
        userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Phantom",
        injected: true,
      }),
    ).toBe(false);
  });

  it("does not need browse on desktop", () => {
    expect(needsInWalletBrowse({ userAgent: "Mozilla/5.0 (Windows NT 10.0)", injected: false })).toBe(false);
  });

  it("builds Phantom and Solflare browse links for this origin", () => {
    const href = "https://example.com/m/abc";
    const origin = "https://example.com";
    expect(phantomBrowseUrl(href, origin)).toBe(
      `https://phantom.app/ul/browse/${encodeURIComponent(href)}?ref=${encodeURIComponent(origin)}`,
    );
    expect(solflareBrowseUrl(href)).toBe(`https://solflare.com/ul/v1/browse/${encodeURIComponent(href)}`);
  });

  it("reads injected Phantom / Solflare", () => {
    expect(readInjectedWallet({ phantom: { solana: {} } })).toBe(true);
    expect(readInjectedWallet({ solana: { isPhantom: true } })).toBe(true);
    expect(readInjectedWallet({ solflare: {} })).toBe(true);
    expect(readInjectedWallet({})).toBe(false);
  });
});
