import express from "express";

const app = express();

app.get("/preview", async (req, res) => {
  const response = await fetch(req.query.url as string);
  res.send(await response.text());
});

app.listen(3000);
