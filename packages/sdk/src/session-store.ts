/** IndexedDB + WebCrypto wrap. Plaintext localStorage is forbidden (FR-WAL-08). */

const DB = "cpm-session";
const STORE = "keys";
const WRAP = "wrap";

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE);
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function wrappingKey(): Promise<CryptoKey> {
  const db = await openDb();
  const existing = await new Promise<ArrayBuffer | undefined>((resolve, reject) => {
    const tx = db.transaction(STORE, "readonly");
    const g = tx.objectStore(STORE).get(WRAP);
    g.onsuccess = () => resolve(g.result);
    g.onerror = () => reject(g.error);
  });
  if (existing) {
    return crypto.subtle.importKey("raw", existing, "AES-GCM", false, ["encrypt", "decrypt"]);
  }
  const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]);
  const raw = await crypto.subtle.exportKey("raw", key);
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(raw, WRAP);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
  return crypto.subtle.importKey("raw", raw, "AES-GCM", false, ["encrypt", "decrypt"]);
}

export async function saveSessionSecret(owner: string, secret: Uint8Array): Promise<void> {
  const key = await wrappingKey();
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const pt = secret.buffer.slice(secret.byteOffset, secret.byteOffset + secret.byteLength) as ArrayBuffer;
  const ct = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, pt));
  const db = await openDb();
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put({ iv: Array.from(iv), ct: Array.from(ct) }, `sess:${owner}`);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

export async function loadSessionSecret(owner: string): Promise<Uint8Array | null> {
  const db = await openDb();
  const row = await new Promise<{ iv: number[]; ct: number[] } | undefined>((resolve, reject) => {
    const tx = db.transaction(STORE, "readonly");
    const g = tx.objectStore(STORE).get(`sess:${owner}`);
    g.onsuccess = () => resolve(g.result);
    g.onerror = () => reject(g.error);
  });
  if (!row) return null;
  const key = await wrappingKey();
  const pt = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv: new Uint8Array(row.iv) },
    key,
    new Uint8Array(row.ct),
  );
  return new Uint8Array(pt);
}

export async function deleteSessionSecret(owner: string): Promise<void> {
  const db = await openDb();
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).delete(`sess:${owner}`);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}
