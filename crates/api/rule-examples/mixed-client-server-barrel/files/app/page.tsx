import { Button, getUser } from "../components";

export default async function AccountPage() {
  const user = await getUser();
  return <Button label={user.name} />;
}
