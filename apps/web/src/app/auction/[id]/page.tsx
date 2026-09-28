import { AuctionDesk } from "@/components/auction-desk";

export default async function AuctionPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return <AuctionDesk market={id} />;
}
