const cors = (options: object): object => options;
const jwt = {
  sign(payload: object, secret: string, options: object): string {
    return JSON.stringify({ payload, secret, options });
  },
};

export const localCors = cors({
  origin: "*",
  credentials: true,
});

export const localJwt = jwt.sign({ sub: "1" }, "ignored", {
  algorithm: "none",
});
