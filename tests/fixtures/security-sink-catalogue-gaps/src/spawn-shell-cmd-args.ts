// An untrusted command and an untrusted argument in one shell call give one
// candidate: the command row already covers the call.
import express from "express";
import { spawn } from "node:child_process";

const app = express();

app.get("/run", (req, res) => {
  spawn(req.query.cmd as string, [req.query.ref as string], { shell: true });
  res.end();
});
