import express from "express";
import { execFile } from "node:child_process";

const app = express();

app.post("/run", (req, res) => {
  execFile(String(req.query.tool), []);
  res.end();
});
