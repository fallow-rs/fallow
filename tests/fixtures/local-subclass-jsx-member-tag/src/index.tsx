import { Base } from './base';

class Sub extends Base {}

const Inner = () => {
  const Sub = { Shadowed: () => null };
  return <Sub.Shadowed />;
};

export const App = () => (
  <>
    <Sub.Item />
    <Inner />
  </>
);
