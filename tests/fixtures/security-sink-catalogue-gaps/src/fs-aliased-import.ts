import express from "express";
import { readFile as read } from "node:fs/promises";

const app = express();

app.get("/files", async (req, res) => {
  res.send(await read(String(req.query.name), "utf8"));
});
