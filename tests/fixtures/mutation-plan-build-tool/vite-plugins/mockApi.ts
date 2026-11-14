import type { Plugin } from "vite";
import { items } from "./items";

export const mockApi = (): Plugin => ({
  name: "mock-api",
  configureServer(server) {
    server.middlewares.use("/api/items", (req, res) => {
      const list = req.method === "GET" ? items() : [];
      res.end(JSON.stringify(list.length > 0 ? list : { error: "none" }));
    });
  },
});
