// Same mis-positioned directive as the `misplaced-directive` fixture, but this
// project does NOT declare `next`. Without an RSC framework the directive has
// no special meaning, so the detector is gated off and emits ZERO findings.
import { helper } from "./helper";

"use client";

export default function Page() {
  return <div>{helper}</div>;
}
