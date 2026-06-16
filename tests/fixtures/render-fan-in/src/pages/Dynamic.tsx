import * as Lib from "../components/Button";

const spreadProps = { label: "dynamic" };

// UNDERCOUNT case (the safe, documented direction): the Button component is
// rendered ONLY via a member-expression tag `<Lib.Button/>`. The shared
// ChildResolver returns None on the dotted name, so this site is NOT credited
// to Button. Button's render-SITES therefore stay 6, never 7. A JSX spread is
// also applied to prove a spread does not silently add a phantom site.
export const Dynamic = () => {
  return <Lib.Button {...spreadProps} />;
};
