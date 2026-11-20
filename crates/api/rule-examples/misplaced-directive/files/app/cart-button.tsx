import { useState } from "react";
"use client";

export function CartButton() {
  const [count, setCount] = useState(0);
  return <button onClick={() => setCount(count + 1)}>Items: {count}</button>;
}
