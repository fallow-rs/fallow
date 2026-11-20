"use client";

export function Button({ label }: { label: string }) {
  return <button onClick={() => alert(label)}>{label}</button>;
}
