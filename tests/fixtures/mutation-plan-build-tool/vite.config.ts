import { defineConfig } from "vite";
import { mockApi } from "./vite-plugins/mockApi";
import { banner } from "./vite-plugins/banner";
import { stamp } from "./vite-plugins/stamp";

const port = 3000 + 1;

export default defineConfig({
  plugins: [
    mockApi(),
    banner(),
    stamp(),
    {
      name: "inline-mock",
      configureServer(server) {
        server.middlewares.use("/api/ping", (req, res) => {
          res.end(req.method === "GET" ? "pong" : "nope");
        });
      },
    },
  ],
  server: { port },
});
