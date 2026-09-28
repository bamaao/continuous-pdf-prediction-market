const nonces = new Map<string, { nonce: string; exp: number }>();

export function putNonce(address: string, nonce: string) {
  nonces.set(address, { nonce, exp: Date.now() + 5 * 60_000 });
}

export function takeNonce(address: string, nonce: string): boolean {
  const row = nonces.get(address);
  if (!row || row.nonce !== nonce || Date.now() > row.exp) return false;
  nonces.delete(address);
  return true;
}
