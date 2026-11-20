import express from "express";
import { Pool } from "pg";

const app = express();
const db = new Pool();

app.get("/users/:id", async (req, res) => {
  const result = await db.query("SELECT * FROM users WHERE id = " + req.params.id);
  res.json(result.rows);
});

app.listen(3000);
