"use client";

import { hydrateListings } from "@cpm/sdk";
import { useEffect } from "react";
import { MARKET_API } from "@/lib/env";

/** Push cached listing names back to the API after a restart. */
export function ListingHydrate() {
  useEffect(() => {
    hydrateListings(MARKET_API).catch(() => undefined);
  }, []);
  return null;
}
