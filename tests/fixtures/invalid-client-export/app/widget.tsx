"use client";

// Negative control: a "use client" file exporting only `default` plus an
// ordinary hook name. Neither is in the illegal set, so no finding.
export function useThing() {
  return 1;
}

export default function Widget() {
  return <div>Widget</div>;
}
