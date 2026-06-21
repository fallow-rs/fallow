import { useState } from "react";
import { DrawableImage } from "./DrawableImage";
import { theme } from "../theme";

/** Screenshot a URL (fresh load) and annotate it. */
export const AnnotateCanvas = () => {
  const [url, setUrl] = useState("http://localhost:5273");
  const [img, setImg] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

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

  return (
    <div
      style={{ padding: 12, width: "100%", height: "100%", overflow: "auto", color: theme.text }}
    >
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
      {img && <DrawableImage dataUrl={img} target={url} onDone={() => setImg(null)} />}
    </div>
  );
};
