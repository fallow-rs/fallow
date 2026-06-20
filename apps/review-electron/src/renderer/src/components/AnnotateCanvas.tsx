import { useEffect, useRef, useState, type MouseEvent } from "react";
import { theme } from "../theme";

/** Screenshot the app-under-review, draw on it, and send the annotation to the agent feed. */
export const AnnotateCanvas = () => {
  const [url, setUrl] = useState("http://localhost:5273");
  const [img, setImg] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const drawing = useRef(false);

  const capture = async (): Promise<void> => {
    setStatus("capturing…");
    try {
      const shot = await window.fallow.capture(url);
      setImg(shot.dataUrl);
      setStatus(null);
    } catch (e) {
      setStatus(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    if (!img) return;
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    const image = new Image();
    image.onload = () => {
      canvas.width = image.width;
      canvas.height = image.height;
      ctx.drawImage(image, 0, 0);
    };
    image.src = img;
  }, [img]);

  const at = (e: MouseEvent<HTMLCanvasElement>): [number, number] => {
    const rect = e.currentTarget.getBoundingClientRect();
    const sx = e.currentTarget.width / rect.width;
    const sy = e.currentTarget.height / rect.height;
    return [(e.clientX - rect.left) * sx, (e.clientY - rect.top) * sy];
  };

  const start = (e: MouseEvent<HTMLCanvasElement>): void => {
    const ctx = canvasRef.current?.getContext("2d");
    if (!ctx) return;
    drawing.current = true;
    const [x, y] = at(e);
    ctx.strokeStyle = theme.danger;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(x, y);
  };
  const move = (e: MouseEvent<HTMLCanvasElement>): void => {
    const ctx = canvasRef.current?.getContext("2d");
    if (!drawing.current || !ctx) return;
    const [x, y] = at(e);
    ctx.lineTo(x, y);
    ctx.stroke();
  };
  const end = (): void => {
    drawing.current = false;
  };

  const save = async (): Promise<void> => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    await window.fallow.saveShot({
      annotatedDataUrl: canvas.toDataURL("image/png"),
      note,
      target: url,
    });
    setStatus("saved to agent feed");
    setNote("");
  };

  return (
    <div style={{ padding: 12, width: "100%", height: "100%", overflow: "auto", color: theme.text }}>
      <div style={{ display: "flex", gap: 6, marginBottom: 8 }}>
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          style={{ flex: 1, fontSize: 12 }}
        />
        <button onClick={() => void capture()} style={{ fontSize: 12 }}>
          Screenshot
        </button>
      </div>
      {status && <p style={{ fontSize: 11, color: theme.muted }}>{status}</p>}
      {img && (
        <>
          <canvas
            ref={canvasRef}
            onMouseDown={start}
            onMouseMove={move}
            onMouseUp={end}
            onMouseLeave={end}
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
          </div>
        </>
      )}
    </div>
  );
};
