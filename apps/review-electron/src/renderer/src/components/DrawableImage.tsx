import { useEffect, useRef, useState, type MouseEvent } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

type Props = { dataUrl: string; target: string; onDone?: () => void };

const canvasPoint = (e: MouseEvent<HTMLCanvasElement>): [number, number] => {
  const rect = e.currentTarget.getBoundingClientRect();
  return [
    (e.clientX - rect.left) * (e.currentTarget.width / rect.width),
    (e.clientY - rect.top) * (e.currentTarget.height / rect.height),
  ];
};

const strokeColor = (): string => {
  const value = getComputedStyle(document.documentElement).getPropertyValue("--destructive").trim();
  return value || "#f87171";
};

/** Draw freehand annotations on an image and send the result to the agent feed. */
export const DrawableImage = ({ dataUrl, target, onDone }: Props) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const drawing = useRef(false);
  const [note, setNote] = useState("");
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    const image = new Image();
    image.addEventListener("load", () => {
      canvas.width = image.width;
      canvas.height = image.height;
      ctx.drawImage(image, 0, 0);
    });
    image.src = dataUrl;
  }, [dataUrl]);

  const startDraw = (e: MouseEvent<HTMLCanvasElement>): void => {
    const ctx = canvasRef.current?.getContext("2d");
    if (!ctx) return;
    drawing.current = true;
    const [x, y] = canvasPoint(e);
    ctx.strokeStyle = strokeColor();
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(x, y);
  };
  const moveDraw = (e: MouseEvent<HTMLCanvasElement>): void => {
    const ctx = canvasRef.current?.getContext("2d");
    if (!drawing.current || !ctx) return;
    const [x, y] = canvasPoint(e);
    ctx.lineTo(x, y);
    ctx.stroke();
  };
  const endDraw = (): void => {
    drawing.current = false;
  };

  const save = async (): Promise<void> => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    await window.fallow.saveShot({ annotatedDataUrl: canvas.toDataURL("image/png"), note, target });
    setStatus("saved to agent feed");
    setNote("");
    onDone?.();
  };

  return (
    <div className="overflow-auto p-2">
      <canvas
        ref={canvasRef}
        onMouseDown={startDraw}
        onMouseMove={moveDraw}
        onMouseUp={endDraw}
        onMouseLeave={endDraw}
        className="max-w-full cursor-crosshair rounded-md border border-border"
      />
      <div className="mt-2 flex gap-1.5">
        <Input
          value={note}
          onChange={(e) => setNote(e.target.value)}
          placeholder="note for the agent"
          className="h-7 text-xs"
        />
        <Button size="sm" className="h-7 text-xs lowercase" onClick={() => void save()}>
          save annotation
        </Button>
        {onDone && (
          <Button size="sm" variant="outline" className="h-7 text-xs lowercase" onClick={onDone}>
            back
          </Button>
        )}
      </div>
      {status && <p className="mt-1.5 text-[11px] text-muted-foreground">{status}</p>}
    </div>
  );
};
