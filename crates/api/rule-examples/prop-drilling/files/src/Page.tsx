import { Layout } from "./Layout";

export const Page = ({ user }: { user: { name: string } }) => <Layout user={user} />;
