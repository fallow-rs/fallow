import express from "express";

const app = express();

app.get("/users", (req, res) => {
  const valid = /^(a+)+$/.test(req.query.name as string);
  res.json({ valid });
});

app.listen(3000);
