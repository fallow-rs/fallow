// The module imports node:fs and reads request input, but `readFile` is a
// local helper.
import express from "express";
import { existsSync } from "node:fs";

function readFile(file: string): string {
  return existsSync(file) ? file : "";
}

const app = express();

app.get("/files", (req, res) => {
  res.send(readFile(String(req.query.name)));
});
