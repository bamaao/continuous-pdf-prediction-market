"use client";

import { WalletAdapterNetwork } from "@solana/wallet-adapter-base";
import { ConnectionProvider, WalletProvider } from "@solana/wallet-adapter-react";
import { WalletModalProvider } from "@solana/wallet-adapter-react-ui";
import { BackpackWalletAdapter } from "@solana/wallet-adapter-backpack";
import { PhantomWalletAdapter } from "@solana/wallet-adapter-phantom";
import { SolflareWalletAdapter } from "@solana/wallet-adapter-solflare";
import { useMemo } from "react";
import { CHAIN_ID, RPC_URL } from "@/lib/env";
import { LocalnetWalletAdapter } from "@/lib/localnet-wallet";
import "@solana/wallet-adapter-react-ui/styles.css";

function adapterNetwork(): WalletAdapterNetwork {
  const id = CHAIN_ID.toLowerCase();
  if (id === "mainnet-beta" || id === "mainnet") return WalletAdapterNetwork.Mainnet;
  if (id === "testnet") return WalletAdapterNetwork.Testnet;
  // localnet / devnet / unknown → Devnet so Solflare does not force Mainnet deep-links
  return WalletAdapterNetwork.Devnet;
}

export function Providers({ children }: { children: React.ReactNode }) {
  const network = useMemo(() => adapterNetwork(), []);
  const wallets = useMemo(
    () => [
      new PhantomWalletAdapter(),
      new SolflareWalletAdapter({ network }),
      new BackpackWalletAdapter(),
      new LocalnetWalletAdapter(),
    ],
    [network],
  );
  return (
    <ConnectionProvider endpoint={RPC_URL}>
      <WalletProvider wallets={wallets} autoConnect>
        <WalletModalProvider>{children}</WalletModalProvider>
      </WalletProvider>
    </ConnectionProvider>
  );
}
