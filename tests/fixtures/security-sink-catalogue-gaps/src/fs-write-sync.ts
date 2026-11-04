import express from "express";
import * as fs from "node:fs";

const app = express();

app.post("/files", (req, res) => {
  fs.writeFileSync(String(req.query.name), "data");
  res.end();
});
