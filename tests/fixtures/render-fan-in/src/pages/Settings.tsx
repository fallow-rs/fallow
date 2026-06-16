import { Button } from "../components/Button";
import { RareModal } from "../components/RareModal";

// Renders <Button> twice (2 sites) and <RareModal> once (1 site).
export const Settings = () => {
  return (
    <div>
      <Button label="apply" />
      <Button label="discard" />
      <RareModal open={true} />
    </div>
  );
};
