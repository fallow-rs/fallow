import { useEffect, useState } from "react";
import type { AgentBackend } from "../../../main/backends";
import { Button } from "@/components/ui/button";

/** Pick a coding-agent backend (codiff-style) and run a grounded agent review. */
export const AgentPanel = () => {
  const [backends, setBackends] = useState<AgentBackend[]>([]);
  const [selected, setSelected] = useState<string>("");
  const [status, setStatus] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    void Promise.all([window.fallow.listBackends(), window.fallow.getConfig()]).then(([b, cfg]) => {
      setBackends(b);
      setSelected(b.some((x) => x.id === cfg.agentBackend) ? cfg.agentBackend : (b[0]?.id ?? ""));
    });
  }, []);

  const run = async (): Promise<void> => {
    setRunning(true);
    setStatus("running agent…");
    try {
      const result = await window.fallow.runAgent(selected);
      setStatus(
        result.ok ? "agent judgments validated against the graph" : `error: ${result.error}`,
      );
    } finally {
      setRunning(false);
    }
  };

  if (backends.length === 0) return null;

  return (
    <section className="mb-4">
      <h3 className="mb-1 text-xs font-medium lowercase">agent review</h3>
      <div className="flex flex-wrap gap-1">
        {backends.map((b) => (
          <Button
            key={b.id}
            size="sm"
            variant={selected === b.id ? "secondary" : "ghost"}
            className="h-6 text-[11px] lowercase"
            onClick={() => setSelected(b.id)}
          >
            {b.label}
          </Button>
        ))}
      </div>
      <Button
        size="sm"
        className="mt-1.5 h-7 text-xs lowercase"
        disabled={running || !selected}
        onClick={() => void run()}
      >
        {running ? "running…" : "run agent review"}
      </Button>
      {status && <p className="mt-1 text-[11px] text-muted-foreground">{status}</p>}
    </section>
  );
};
