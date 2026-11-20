import express from "express";
import { MongoClient } from "mongodb";

const app = express();
const users = new MongoClient("mongodb://localhost:27017").db("shop").collection("users");
app.use(express.json());

app.post("/login", async (req, res) => {
  const user = await users.findOne(req.body);
  res.sendStatus(user ? 204 : 401);
});

app.listen(3000);
