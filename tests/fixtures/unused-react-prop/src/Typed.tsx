// Feature A: a NON-exported component whose first param is a bare identifier
// typed by a SAME-FILE `interface` object literal. `props.size` is read (used);
// `typedDead` is harvested from the interface and read NOWHERE -> a NEW true
// positive the typed-interface harvest unlocks. The exported `Typed` wrapper
// (public contract) abstains its own props.
interface InnerProps {
  size: number;
  typedDead: string;
}

const TypedInner = (props: InnerProps) => <div style={{ width: props.size }} />;

export const Typed = ({ heading }: { heading: string }) => (
  <section>
    {heading}
    <TypedInner size={4} typedDead="x" />
  </section>
);
