import express from "express";
import libxmljs from "libxmljs";

const app = express();
app.use(express.text({ type: "application/xml" }));

app.post("/orders/import", (req, res) => {
  const doc = libxmljs.parseXml(req.body, { noent: true });
  res.json({ orders: doc.find("//order").length });
});

app.listen(3000);
