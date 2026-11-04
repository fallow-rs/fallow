import express from "express";

const app = express();

app.get("/files", (req, res) => {
  res.download(String(req.query.name));
});
