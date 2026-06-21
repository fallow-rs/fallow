import { useState } from "react";
import { CheckCircle2, ChevronDown, ChevronRight } from "lucide-react";
import type { ClearedItem } from "../../../model/walkthrough";

export const ClearedPanel = ({ cleared }: { cleared: ClearedItem[] }) => {
  const [open, setOpen] = useState(false);
  if (cleared.length === 0) return null;
  const total = cleared.reduce((n, c) => n + c.count, 0);
  return (
    <section>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-1.5 rounded-md border border-border bg-muted/30 px-2.5 py-2 text-xs text-muted-foreground outline-none transition-colors hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/60"
      >
        {open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
        <CheckCircle2 className="size-3.5 text-fallow-green" />
        <span>
          fallow handled <span className="font-mono tabular-nums text-foreground">{total}</span>{" "}
          technical items
        </span>
      </button>
      {open && (
        <ul className="mt-1.5 space-y-1 px-2.5 text-xs text-muted-foreground">
          {cleared.map((c) => (
            <li key={c.kind} className="flex items-center gap-2">
              <span className="font-mono tabular-nums text-foreground">{c.count}</span>
              {c.label}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
};
