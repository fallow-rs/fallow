"use client";
import dynamic from "next/dynamic";

// `server-mod` imports node:fs (server-only) but is reached ONLY through a
// next/dynamic ssr:false dynamic import, the sanctioned client-only escape
// hatch. Because next/dynamic uses a dynamic `import()` (not a static import),
// the edge is never in the static cone, so no server-only finding fires.
const ServerMod = dynamic(() => import("./server-mod"), { ssr: false });

export function DynamicView() {
  return ServerMod;
}
