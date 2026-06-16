import { Page } from "./Page";

// Entry point: owns the data and starts the drilling chain.
export const App = () => {
  const user = { name: "Ada" };
  return <Page user={user} />;
};
