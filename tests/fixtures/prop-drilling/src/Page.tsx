import { Layout } from "./Layout";

// SOURCE hop: receives `user` and ONLY forwards it (pass-through).
export const Page = ({ user }: { user: { name: string } }) => (
  <Layout user={user} />
);
