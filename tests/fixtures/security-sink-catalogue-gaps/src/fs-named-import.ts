import express from "express";
import { readFileSync } from "node:fs";

const app = express();

app.get("/files", (req, res) => {
  res.send(readFileSync(String(req.query.name), "utf8"));
});
