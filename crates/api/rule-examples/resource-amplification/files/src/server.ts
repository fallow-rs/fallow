import express from "express";

const app = express();

app.get("/coupons", (req, res) => {
  const coupons = new Array(Number(req.query.count));
  res.json({ size: coupons.length });
});

app.listen(3000);
