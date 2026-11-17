// The second argument is the options object, not an argument array. Its `cwd`
// value does not reach the shell command line.
import express from "express";
import { execFile } from "node:child_process";

const app = express();

app.get("/log", (req, res) => {
  execFile("git", { shell: true, cwd: req.query.dir as string }, () => {
    res.end();
  });
});
