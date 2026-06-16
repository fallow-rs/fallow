import { createContext } from "react";
import { ProviderLeaf } from "./ProviderLeaf";

const UserContext = createContext<{ name: string } | null>(null);

// Rendering `<UserContext.Provider>` marks this component renders_provider, which
// abstains any drilling chain through it.
export const ProviderMid = ({ user }: { user: { name: string } }) => (
  <UserContext.Provider value={user}>
    <ProviderLeaf user={user} />
  </UserContext.Provider>
);
