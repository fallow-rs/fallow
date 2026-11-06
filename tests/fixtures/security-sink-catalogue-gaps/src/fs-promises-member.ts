import express from "express";
import { promises as fsp } from "node:fs";

const app = express();

app.get("/files", async (req, res) => {
  res.send(await fsp.readFile(String(req.query.name), "utf8"));
});
