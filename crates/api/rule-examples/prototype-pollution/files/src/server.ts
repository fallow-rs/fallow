import express from "express";
import _ from "lodash";

const app = express();
const settings = { currency: "EUR", taxRate: 21 };
app.use(express.json());

app.post("/settings", (req, res) => {
  _.merge(settings, req.body);
  res.json(settings);
});

app.listen(3000);
