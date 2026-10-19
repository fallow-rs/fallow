import { MongoClient } from "mongodb";

const client = new MongoClient("mongodb://localhost:27017");

export async function addRecord(value: number): Promise<boolean> {
  if (value < 0) return false;
  await client.db("app").collection("records").insertOne({ value });
  return true;
}
