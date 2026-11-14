import express from "express";
import { spawn } from "node:child_process";

const app = express();

app.post("/spawn", (req, res) => {
  spawn(String(req.query.cmd), []);
  res.end();
});
