import express from "express";
import { writeFile } from "node:fs/promises";

const app = express();

app.post("/files", async (req, res) => {
  await writeFile(String(req.query.name), "data");
  res.end();
});
