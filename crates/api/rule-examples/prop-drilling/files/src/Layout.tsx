import { Sidebar } from "./Sidebar";

export const Layout = ({ user }: { user: { name: string } }) => (
  <div className="layout">
    <Sidebar user={user} />
  </div>
);
