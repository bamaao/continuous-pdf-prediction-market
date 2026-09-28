import type { Metadata } from "next";
import { ListingHydrate } from "@/components/listing-hydrate";
import { Providers } from "@/components/providers";
import { Shell } from "@/components/shell";
import "./globals.css";

export const metadata: Metadata = {
  title: "Continuous — PDF prediction market",
  description: "Continuous PDF prediction market. Fills are public on Solana.",
  manifest: "/manifest.json",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link
          href="https://fonts.googleapis.com/css2?family=Cormorant+Garamond:wght@500;600&family=IBM+Plex+Mono:wght@400;500&display=swap"
          rel="stylesheet"
        />
      </head>
      <body>
        <Providers>
          <ListingHydrate />
          <Shell>{children}</Shell>
        </Providers>
      </body>
    </html>
  );
}
