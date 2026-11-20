import { DOMParser } from "@xmldom/xmldom";
import express from "express";
import xpath from "xpath";

const app = express();
const doc = new DOMParser().parseFromString("<users><user name='guest'/></users>", "text/xml");

app.get("/users", (req, res) => {
  const nodes = xpath.select("//user[name='" + req.query.name + "']", doc);
  res.json({ found: Array.isArray(nodes) && nodes.length > 0 });
});

app.listen(3000);
