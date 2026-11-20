import express from "express";
import fs from "node:fs";
import path from "node:path";

const app = express();

const uploadDir = "/srv/invoices";

app.get("/invoices/:name", (req, res) => {
  fs.readFile(path.join(uploadDir, req.params.name), (error, data) => {
    res.send(error ? "Not found" : data);
  });
});

app.listen(3000);
