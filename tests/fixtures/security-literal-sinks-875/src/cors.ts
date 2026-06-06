import cors from "cors";

export const middleware = cors({
  origin: "*",
  credentials: true,
});
