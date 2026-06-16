import { Button } from "../components/Button";

// Renders <Button> once: 1 render SITE from a third distinct parent.
export const Toolbar = () => {
  return (
    <nav>
      <Button label="menu" />
    </nav>
  );
};
