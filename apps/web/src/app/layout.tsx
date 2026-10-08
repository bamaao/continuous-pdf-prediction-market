import type { Metadata, Viewport } from "next";
import { ListingHydrate } from "@/components/listing-hydrate";
import { Providers } from "@/components/providers";
import { RegisterSw } from "@/components/register-sw";
import { Shell } from "@/components/shell";
import "./globals.css";

export const viewport: Viewport = {
  themeColor: "#12110e",
};

export const metadata: Metadata = {
  title: "Continuous — PDF prediction market",
  description: "Continuous PDF prediction market. Fills are public on Solana.",
  applicationName: "Continuous",
  manifest: "/manifest.json",
  appleWebApp: {
    capable: true,
    title: "Continuous",
    statusBarStyle: "black-translucent",
  },
  icons: {
    icon: [
      { url: "/favicon.ico", sizes: "48x48" },
      { url: "/icon-32.png", sizes: "32x32", type: "image/png" },
      { url: "/icon-192.png", sizes: "192x192", type: "image/png" },
      { url: "/icon-512.png", sizes: "512x512", type: "image/png" },
    ],
    apple: [{ url: "/icon-180.png", sizes: "180x180", type: "image/png" }],
  },
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
          <RegisterSw />
          <ListingHydrate />
          <Shell>{children}</Shell>
        </Providers>
      </body>
    </html>
  );
}
