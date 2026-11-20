import express from "express";

const app = express();

app.use(express.json());

app.post("/cart/discount", (req, res) => {
  const discount = eval(req.body.expression);
  res.json({ discount });
});

app.listen(3000);
