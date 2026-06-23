import { useState, useEffect, useMemo } from "react";

// TWO components in ONE file, each calling DIFFERENT hooks. The per-component
// hook summary must attribute exactly: ComponentA owns the useState + useEffect,
// ComponentB owns the useMemo. Before per-hook component tagging, a multi-component
// file left both summaries empty.
export const ComponentA = () => {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    setOpen(true);
  }, []);
  return <div className="a">{open ? "open" : "closed"}</div>;
};

export const ComponentB = () => {
  const total = useMemo(() => 1 + 2, []);
  return <div className="b">{total}</div>;
};
