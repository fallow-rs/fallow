import express from "express";

const app = express();

app.get("/plugins/:name", (req, res) => {
  const plugin = require("./plugins/" + req.params.name);
  res.json(plugin.describe());
});

app.listen(3000);
