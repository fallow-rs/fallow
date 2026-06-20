import { useEffect, useRef, useState, type MouseEvent } from "react";
import { theme } from "../theme";

type Props = { dataUrl: string; target: string; onDone?: () => void };

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
    image.onload = () => {
      canvas.width = image.width;
      canvas.height = image.height;
      ctx.drawImage(image, 0, 0);
    };
    image.src = dataUrl;
  }, [dataUrl]);

  const at = (e: MouseEvent<HTMLCanvasElement>): [number, number] => {
    const rect = e.currentTarget.getBoundingClientRect();
    return [
      (e.clientX - rect.left) * (e.currentTarget.width / rect.width),
      (e.clientY - rect.top) * (e.currentTarget.height / rect.height),
    ];
  };
  const startDraw = (e: MouseEvent<HTMLCanvasElement>): void => {
    const ctx = canvasRef.current?.getContext("2d");
    if (!ctx) return;
    drawing.current = true;
    const [x, y] = at(e);
    ctx.strokeStyle = theme.danger;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(x, y);
  };
  const moveDraw = (e: MouseEvent<HTMLCanvasElement>): void => {
    const ctx = canvasRef.current?.getContext("2d");
    if (!drawing.current || !ctx) return;
    const [x, y] = at(e);
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
    <div style={{ padding: 8, overflow: "auto" }}>
      <canvas
        ref={canvasRef}
        onMouseDown={startDraw}
        onMouseMove={moveDraw}
        onMouseUp={endDraw}
        onMouseLeave={endDraw}
        style={{ maxWidth: "100%", border: `1px solid ${theme.border}`, cursor: "crosshair" }}
      />
      <div style={{ display: "flex", gap: 6, marginTop: 8 }}>
        <input
          value={note}
          onChange={(e) => setNote(e.target.value)}
          placeholder="note for the agent"
          style={{ flex: 1, fontSize: 12 }}
        />
        <button onClick={() => void save()} style={{ fontSize: 12 }}>
          Save annotation
        </button>
        {onDone && (
          <button onClick={onDone} style={{ fontSize: 12 }}>
            Back
          </button>
        )}
      </div>
      {status && <p style={{ fontSize: 11, color: theme.muted }}>{status}</p>}
    </div>
  );
};
