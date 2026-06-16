import { SpreadPage } from "./spread/SpreadPage";
import { ClonePage } from "./clone/ClonePage";
import { ElementPage } from "./element/ElementPage";
import { ProviderPage } from "./provider/ProviderPage";
import { RenamePage } from "./rename/RenamePage";

// Entry: each chain below is a 3+ hop forward that carries ONE abstain signal,
// so the prop-drilling detector must emit ZERO chains for this project.
export const App = () => {
  const user = { name: "Ada" };
  return (
    <div>
      <SpreadPage user={user} />
      <ClonePage user={user} />
      <ElementPage user={user} />
      <ProviderPage user={user} />
      <RenamePage user={user} />
    </div>
  );
};
