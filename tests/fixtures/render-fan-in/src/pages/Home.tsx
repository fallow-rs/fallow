import { Button } from "../components/Button";

// Renders <Button> 3 times: 3 render SITES from 1 parent component.
export const Home = () => {
  return (
    <div>
      <Button label="save" />
      <Button label="cancel" />
      <Button label="reset" />
    </div>
  );
};
