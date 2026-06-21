import { useState } from "react";
import { DrawableImage } from "./DrawableImage";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

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
    <div className="h-full w-full overflow-auto p-3 text-foreground">
      <div className="mb-2 flex gap-1.5">
        <Input value={url} onChange={(e) => setUrl(e.target.value)} className="h-8 text-xs" />
        <Button size="sm" className="lowercase" onClick={() => void capture()}>
          screenshot
        </Button>
      </div>
      {status && <p className="text-[11px] text-muted-foreground">{status}</p>}
      {img && <DrawableImage dataUrl={img} target={url} onDone={() => setImg(null)} />}
    </div>
  );
};
