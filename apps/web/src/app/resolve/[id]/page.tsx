import { ResolveDesk } from "@/components/resolve-desk";

export default async function ResolvePage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return <ResolveDesk market={id} />;
}
