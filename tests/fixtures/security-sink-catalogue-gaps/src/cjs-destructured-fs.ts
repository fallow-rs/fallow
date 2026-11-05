import express from "express";

const { readFileSync } = require("node:fs");
const app = express();

app.get("/files", (req, res) => {
  res.send(readFileSync(String(req.query.name), "utf8"));
});
