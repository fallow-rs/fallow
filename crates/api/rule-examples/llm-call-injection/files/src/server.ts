import express from "express";
import OpenAI from "openai";

const app = express();
const openai = new OpenAI();
app.use(express.json());

app.post("/support/ask", async (req, res) => {
  const answer = await openai.chat.completions.create({
    model: "gpt-4o-mini",
    messages: [{ role: "user", content: req.body.q }],
  });
  res.json(answer.choices[0].message);
});

app.listen(3000);
