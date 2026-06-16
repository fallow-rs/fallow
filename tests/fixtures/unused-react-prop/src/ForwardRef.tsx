import { forwardRef } from "react";
import type { CaptionProps } from "./types";

// A NON-exported forwardRef whose props come from an imported interface
// (`CaptionProps`, unresolvable per ADR-001): the signature carries a bare
// `props` identifier, so the whole component abstains. `unread` lives in the
// imported interface and must NOT be flagged.
const Inner = forwardRef<HTMLDivElement, CaptionProps>((props, ref) => (
  <div ref={ref}>{props.caption}</div>
));

export const ForwardRef = ({ caption }: { caption: string }) => (
  <Inner caption={caption} unread="z" />
);
