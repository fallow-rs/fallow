import { useState } from "react";
import type { ClearedItem } from "../../../model/walkthrough";
import { Button } from "@/components/ui/button";

export const ClearedPanel = ({ cleared }: { cleared: ClearedItem[] }) => {
  const [open, setOpen] = useState(false);
  if (cleared.length === 0) return null;
  const total = cleared.reduce((n, c) => n + c.count, 0);
  return (
    <section className="mb-4">
      <Button
        variant="outline"
        size="sm"
        className="h-7 text-xs lowercase"
        onClick={() => setOpen((o) => !o)}
      >
        {open ? "▾" : "▸"} fallow already handled{" "}
        <span className="font-mono tabular-nums">{total}</span> technical items
      </Button>
      {open && (
        <ul className="mt-2 list-disc pl-4 text-xs text-muted-foreground">
          {cleared.map((c) => (
            <li key={c.kind}>
              <span className="font-mono tabular-nums">{c.count}</span> {c.label}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
};
