import express from "express";

const app = express();
app.use(express.json());

app.post("/login", (req, res) => {
  console.log("login", req.body.email, req.body.password);
  res.sendStatus(204);
});

app.listen(3000);
