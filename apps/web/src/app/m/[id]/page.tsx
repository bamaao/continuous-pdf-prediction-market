import { Board } from "@/components/board";

export default async function MarketPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return <Board market={id} />;
}
