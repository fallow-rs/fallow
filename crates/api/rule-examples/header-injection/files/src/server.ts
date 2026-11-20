import express from "express";

const app = express();

app.get("/login", (req, res) => {
  res.setHeader("Location", req.query.next as string);
  res.sendStatus(302);
});

app.listen(3000);
