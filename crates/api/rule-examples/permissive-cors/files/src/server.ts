import express from "express";
import cors from "cors";

const app = express();

app.use(cors({ origin: "*", credentials: true }));

app.get("/orders", (req, res) => {
  res.json([]);
});

app.listen(3000);
