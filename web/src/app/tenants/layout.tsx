import { Atmosphere } from "@/components/atmosphere";
export default function Layout({ children }: { children: React.ReactNode }) {
  return (
    <div className="authenticated-spaces">
      <Atmosphere className="workspace-atmosphere" />
      {children}
    </div>
  );
}
