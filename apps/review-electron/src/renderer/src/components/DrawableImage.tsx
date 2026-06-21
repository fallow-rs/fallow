import { useEffect, useRef, useState, type MouseEvent } from "react";
import { Eraser, Undo2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";

type Props = { dataUrl: string; target: string; onDone?: () => void };

type Point = [number, number];
type Stroke = { color: string; points: Point[] };

const SWATCHES = [
  { name: "red", cssVar: "--fallow-red", fallback: "#f87171", bg: "bg-fallow-red" },
  { name: "amber", cssVar: "--fallow-amber", fallback: "#fbbf24", bg: "bg-fallow-amber" },
  { name: "green", cssVar: "--fallow-green", fallback: "#4ade80", bg: "bg-fallow-green" },
  { name: "blue", cssVar: "--fallow-blue", fallback: "#60a5fa", bg: "bg-fallow-blue" },
] as const;

const canvasPoint = (e: MouseEvent<HTMLCanvasElement>): Point => {
  const rect = e.currentTarget.getBoundingClientRect();
  return [
    (e.clientX - rect.left) * (e.currentTarget.width / rect.width),
    (e.clientY - rect.top) * (e.currentTarget.height / rect.height),
  ];
};

const resolveColor = (cssVar: string, fallback: string): string => {
  const value = getComputedStyle(document.documentElement).getPropertyValue(cssVar).trim();
  return value || fallback;
};

/** Draw freehand annotations on an image and send the result to the agent feed. */
export const DrawableImage = ({ dataUrl, target, onDone }: Props) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const strokes = useRef<Stroke[]>([]);
  const drawing = useRef(false);
  const [colorIdx, setColorIdx] = useState(0);
  const [count, setCount] = useState(0);
  const [note, setNote] = useState("");
  const [status, setStatus] = useState<string | null>(null);

  const redraw = (): void => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    const image = imageRef.current;
    if (!canvas || !ctx || !image) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(image, 0, 0);
    ctx.lineWidth = 3;
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    for (const stroke of strokes.current) {
      ctx.strokeStyle = stroke.color;
      ctx.beginPath();
      stroke.points.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.stroke();
    }
  };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    strokes.current = [];
    setCount(0);
    const image = new Image();
    image.addEventListener("load", () => {
      canvas.width = image.width;
      canvas.height = image.height;
      imageRef.current = image;
      redraw();
    });
    image.src = dataUrl;
  }, [dataUrl]);

  const startDraw = (e: MouseEvent<HTMLCanvasElement>): void => {
    drawing.current = true;
    const swatch = SWATCHES[colorIdx] ?? SWATCHES[0];
    strokes.current.push({
      color: resolveColor(swatch.cssVar, swatch.fallback),
      points: [canvasPoint(e)],
    });
  };
  const moveDraw = (e: MouseEvent<HTMLCanvasElement>): void => {
    if (!drawing.current) return;
    const stroke = strokes.current[strokes.current.length - 1];
    if (!stroke) return;
    stroke.points.push(canvasPoint(e));
    redraw();
  };
  const endDraw = (): void => {
    if (drawing.current) setCount(strokes.current.length);
    drawing.current = false;
  };
  const undo = (): void => {
    strokes.current.pop();
    setCount(strokes.current.length);
    redraw();
  };
  const clearAll = (): void => {
    strokes.current = [];
    setCount(0);
    redraw();
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
    <div className="flex h-full flex-col overflow-hidden p-2">
      <div className="mb-2 flex shrink-0 items-center gap-2">
        <div className="flex items-center gap-1.5">
          {SWATCHES.map((s, i) => (
            <button
              key={s.name}
              type="button"
              aria-label={`${s.name} pen`}
              aria-pressed={colorIdx === i}
              onClick={() => setColorIdx(i)}
              className={cn(
                "size-4 rounded-full outline-none ring-offset-2 ring-offset-background transition-shadow focus-visible:ring-2 focus-visible:ring-ring",
                s.bg,
                colorIdx === i ? "ring-2 ring-ring" : "ring-0",
              )}
            />
          ))}
        </div>
        <Separator orientation="vertical" className="h-5" />
        <Button
          size="icon"
          variant="ghost"
          className="size-7 text-muted-foreground"
          aria-label="undo"
          disabled={count === 0}
          onClick={undo}
        >
          <Undo2 className="size-3.5" />
        </Button>
        <Button
          size="icon"
          variant="ghost"
          className="size-7 text-muted-foreground"
          aria-label="clear annotations"
          disabled={count === 0}
          onClick={clearAll}
        >
          <Eraser className="size-3.5" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <canvas
          ref={canvasRef}
          onMouseDown={startDraw}
          onMouseMove={moveDraw}
          onMouseUp={endDraw}
          onMouseLeave={endDraw}
          className="max-w-full cursor-crosshair rounded-md border border-border"
        />
      </div>
      <div className="mt-2 flex shrink-0 gap-1.5">
        <Input
          value={note}
          onChange={(e) => setNote(e.target.value)}
          placeholder="note for the agent"
          className="h-7 text-xs"
        />
        <Button size="sm" className="h-7 text-xs lowercase" onClick={() => void save()}>
          send to agent
        </Button>
        {onDone && (
          <Button size="sm" variant="outline" className="h-7 text-xs lowercase" onClick={onDone}>
            back
          </Button>
        )}
      </div>
      {status && <p className="mt-1.5 shrink-0 text-[11px] text-muted-foreground">{status}</p>}
    </div>
  );
};
