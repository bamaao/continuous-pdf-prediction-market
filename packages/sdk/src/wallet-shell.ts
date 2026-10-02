/** How this site reaches a phone wallet. Injected provider vs browse-in-wallet. */

export function isIosUserAgent(ua: string): boolean {
  return /iPhone|iPad|iPod/i.test(ua) || (/Macintosh/i.test(ua) && /Mobile/i.test(ua));
}

export function isAndroidUserAgent(ua: string): boolean {
  return /Android/i.test(ua);
}

export function isStandaloneDisplay(standalone: boolean, iosStandalone?: boolean): boolean {
  return standalone || iosStandalone === true;
}

export function needsInWalletBrowse(hint: { userAgent: string; injected: boolean }): boolean {
  if (hint.injected) return false;
  return isIosUserAgent(hint.userAgent) || isAndroidUserAgent(hint.userAgent);
}

export function phantomBrowseUrl(href: string, origin: string): string {
  return `https://phantom.app/ul/browse/${encodeURIComponent(href)}?ref=${encodeURIComponent(origin)}`;
}

export function solflareBrowseUrl(href: string): string {
  return `https://solflare.com/ul/v1/browse/${encodeURIComponent(href)}`;
}

export function readInjectedWallet(win: object): boolean {
  const w = win as {
    solana?: { isPhantom?: boolean; isSolflare?: boolean };
    phantom?: { solana?: unknown };
    solflare?: unknown;
  };
  return Boolean(w.solana?.isPhantom || w.solana?.isSolflare || w.phantom?.solana || w.solflare);
}
