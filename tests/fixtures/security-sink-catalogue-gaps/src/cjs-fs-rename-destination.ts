import express from "express";

const fs = require("node:fs");
const app = express();

app.post("/move", (req, res) => {
  fs.rename("/srv/uploads/tmp", String(req.query.name), () => res.end());
});
