interface RequestLike {
  body: {
    email: string;
    token: string;
  };
}

const logger = {
  error(value: unknown): void {
    void value;
  },
  warn(value: unknown): void {
    void value;
  },
};

const route = {
  logger,
};

export function logSecrets(req: RequestLike): void {
  const email = req.body.email;
  const { token } = req.body;
  const secret = process.env.SECRET_KEY;

  console.log(email);
  console.warn(token);
  logger.error(secret);
  route.logger.warn(process.env.API_TOKEN);
}
