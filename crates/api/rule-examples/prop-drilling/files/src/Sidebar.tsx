import { Profile } from "./Profile";

export const Sidebar = ({ user }: { user: { name: string } }) => (
  <aside>
    <Profile user={user} />
  </aside>
);
