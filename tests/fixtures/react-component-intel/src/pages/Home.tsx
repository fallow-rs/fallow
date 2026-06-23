import { Card } from "../components/Card";

// Renders <Card> 3 times: 3 render SITES from 1 parent component. `title` is
// passed at all 3 sites; `subtitle` at only 1.
export const Home = () => {
  return (
    <div>
      <Card title="a" subtitle="x" />
      <Card title="b" />
      <Card title="c" />
    </div>
  );
};
