import express from "express";
import * as cp from "node:child_process";

const app = express();

app.get("/log", (req, res) => {
  cp.execFileSync("git", ["log", String(req.query.ref)], { shell: "/bin/sh" });
  res.end();
});
