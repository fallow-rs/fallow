import express from "express";
import { randomUUID } from "node:crypto";

const app = express();

app.post("/login", (req, res) => {
  const id = randomUUID();
  res.cookie("session", id, { httpOnly: false });
  res.sendStatus(204);
});

app.listen(3000);
