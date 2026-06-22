import { forwardRef } from "react";

// Feature A v2: a NON-exported `forwardRef<Ref, Props>` whose props type is the
// SECOND generic argument resolving to a SAME-FILE `interface`. `props.size` is
// read (used); `genericDead` is harvested from the interface and read NOWHERE ->
// a NEW true positive the generic-forwardRef harvest unlocks. The inner `props`
// param carries no annotation; the type comes from the wrapper generic.
interface GenericProps {
  size: number;
  genericDead: string;
}

const GenericInner = forwardRef<HTMLDivElement, GenericProps>((props, ref) => (
  <div ref={ref} style={{ width: props.size }} />
));

export const ForwardRefGeneric = ({ heading }: { heading: string }) => (
  <section>
    {heading}
    <GenericInner size={4} genericDead="x" />
  </section>
);
