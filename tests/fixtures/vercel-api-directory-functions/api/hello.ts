import { greeting } from "./_lib/util";

export const config = { runtime: "nodejs" };

export default function handler(req: unknown, res: { end: (body: string) => void }) {
  res.end(greeting());
}
