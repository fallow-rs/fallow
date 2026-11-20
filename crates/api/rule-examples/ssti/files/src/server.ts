import ejs from "ejs";
import express from "express";

const app = express();

app.get("/products", (req, res) => {
  res.send(ejs.render("<h1>" + req.query.title + "</h1>"));
});

app.listen(3000);
