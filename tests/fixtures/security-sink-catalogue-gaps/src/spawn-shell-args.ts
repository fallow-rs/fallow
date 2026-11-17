// With `shell: true`, spawn joins the command and the arguments into one
// shell command line.
import express from "express";
import { spawn } from "node:child_process";

const app = express();

app.get("/log", (req, res) => {
  spawn("git", [req.query.ref as string], { shell: true });
  res.end();
});
