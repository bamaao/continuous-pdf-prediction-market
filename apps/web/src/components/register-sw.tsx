"use client";

import { useEffect } from "react";

/** Register the PWA service worker once on the client. Never in Next.js dev — it caches webpack chunks. */
export function RegisterSw() {
  useEffect(() => {
    if (typeof window === "undefined" || !("serviceWorker" in navigator)) return;
    if (process.env.NODE_ENV !== "production") {
      void navigator.serviceWorker.getRegistrations().then((regs) => Promise.all(regs.map((r) => r.unregister())));
      if ("caches" in window) {
        void caches.keys().then((keys) => Promise.all(keys.map((k) => caches.delete(k))));
      }
      return;
    }
    navigator.serviceWorker.register("/sw.js").catch(() => undefined);
  }, []);
  return null;
}
