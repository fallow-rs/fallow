// With `shell: true`, execFile runs the command line through a shell.
import express from "express";
import { execFile } from "node:child_process";

const app = express();

app.get("/log", (req, res) => {
  execFile("git", ["log", req.query.ref as string], { shell: true }, () => {
    res.end();
  });
});
