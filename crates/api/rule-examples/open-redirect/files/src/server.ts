import express from "express";

const app = express();

app.get("/logout", (req, res) => {
  res.redirect(req.query.returnTo as string);
});

app.listen(3000);
