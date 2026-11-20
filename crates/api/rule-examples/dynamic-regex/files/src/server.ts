import express from "express";

const app = express();

app.get("/orders", (req, res) => {
  const pattern = new RegExp(req.query.filter as string);
  res.json({ matches: pattern.test("order-1001") });
});

app.listen(3000);
