import express from "express";
import yaml from "js-yaml";

const app = express();
app.use(express.json());

app.post("/shop/config", (req, res) => {
  const config = yaml.load(req.body.config);
  res.json(config);
});

app.listen(3000);
