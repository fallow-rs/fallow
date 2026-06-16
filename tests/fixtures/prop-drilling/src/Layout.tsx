import { Sidebar } from "./Sidebar";

// PASS-THROUGH hop 1: `user` is received and only re-passed.
export const Layout = ({ user }: { user: { name: string } }) => (
  <div className="layout">
    <Sidebar user={user} />
  </div>
);
