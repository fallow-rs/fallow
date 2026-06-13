"use client";

// A "use client" module that also exports a type. A type-only re-export of this
// module carries no runtime directive context.
export type ButtonProps = { label: string };

export function TypedButton() {
  return null;
}
