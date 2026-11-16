// Without a shell, spawn passes each argument to the program unchanged.
import express from "express";
import { execFile, spawn } from "node:child_process";

const app = express();

app.get("/log", (req, res) => {
  spawn("git", ["log", req.query.ref as string]);
  execFile("git", ["log", req.query.ref as string], { shell: false });
  res.end();
});
