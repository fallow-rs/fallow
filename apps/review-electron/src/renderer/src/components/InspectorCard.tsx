import type { InspectorCard as Card } from "../../../main/inspect";
import { Card as UiCard, CardContent } from "@/components/ui/card";

export const InspectorCard = ({ card }: { card: Card }) => (
  <UiCard className="mb-3 gap-0 border-primary/40 py-0">
    <CardContent className="p-2.5">
      <div className="text-[11px] text-muted-foreground lowercase">inspected</div>
      <div className="font-mono text-xs">
        {card.component ? `${card.component} · ` : ""}
        {card.file}:<span className="tabular-nums">{card.line}</span>
      </div>
      <ul className="mt-1.5 list-disc pl-4 text-[11px] text-muted-foreground">
        {card.facts.map((f, i) => (
          <li key={`${card.file}-${i}`}>{f}</li>
        ))}
      </ul>
    </CardContent>
  </UiCard>
);
