import { CommitteeDesk } from "@/components/committee-desk";

export const dynamic = "force-dynamic";

export default async function Committee({ searchParams }: { searchParams: Promise<{ market?: string }> }) {
  const { market } = await searchParams;
  return <CommitteeDesk initialMarket={market ?? ""} />;
}
