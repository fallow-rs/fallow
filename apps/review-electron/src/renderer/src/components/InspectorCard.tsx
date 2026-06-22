import { MousePointerClick } from "lucide-react";
import type { InspectorCard as Card } from "../../../main/inspect";
import { Card as UiCard, CardContent } from "@/components/ui/card";

export const InspectorCard = ({ card }: { card: Card }) => (
  <UiCard data-testid="inspector-card" className="gap-0 border-primary/40 bg-primary/5 py-0">
    <CardContent className="space-y-1.5 p-3">
      <div className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        <MousePointerClick className="size-3.5" />
        inspected
      </div>
      <div className="font-mono text-xs">
        {card.component ? `${card.component} · ` : ""}
        {card.file}:<span className="tabular-nums">{card.line}</span>
      </div>
      <ul className="space-y-0.5 text-[11px] text-muted-foreground">
        {card.facts.map((f, i) => (
          <li key={`${card.file}-${i}`} className="flex gap-1.5">
            <span className="text-muted-foreground/50">•</span>
            {f}
          </li>
        ))}
      </ul>
    </CardContent>
  </UiCard>
);
