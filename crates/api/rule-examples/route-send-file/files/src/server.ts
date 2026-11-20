import express from "express";

const app = express();

app.get("/invoices/:file", (req, res) => {
  res.sendFile(req.params.file, { root: "public" });
});

app.listen(3000);
