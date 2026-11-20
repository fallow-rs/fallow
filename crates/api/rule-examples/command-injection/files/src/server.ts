import express from "express";
import child_process from "node:child_process";

const app = express();

app.get("/thumbnails", (req, res) => {
  child_process.exec("convert " + req.query.file + " out.png");
  res.sendStatus(202);
});

app.listen(3000);
