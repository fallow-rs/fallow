"use client";

import { useState } from "react";

export const metadata = { title: "Cart" };

export default function CartPage() {
  const [count, setCount] = useState(0);
  return <button onClick={() => setCount(count + 1)}>Items: {count}</button>;
}
