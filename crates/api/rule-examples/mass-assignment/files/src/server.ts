import express from "express";

interface User {
  name: string;
  role: string;
}

const app = express();
const users = new Map<string, User>();
app.use(express.json());

app.patch("/users/:id", (req, res) => {
  const user = users.get(req.params.id) ?? { name: "", role: "customer" };
  Object.assign(user, req.body);
  users.set(req.params.id, user);
  res.json(user);
});

app.listen(3000);
