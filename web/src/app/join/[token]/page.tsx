import { InvitationJoin } from "@/components/tenant-chooser";

export default async function Page({ params }: {
  params: Promise<{ token: string }>;
}) {
  const { token } = await params;
  return <InvitationJoin token={token} />;
}
