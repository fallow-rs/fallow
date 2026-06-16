import { Profile } from "./Profile";

// PASS-THROUGH hop 2: `user` is received and only re-passed.
export const Sidebar = ({ user }: { user: { name: string } }) => (
  <aside>
    <Profile user={user} />
  </aside>
);
