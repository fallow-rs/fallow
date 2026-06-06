const logger = {
  info(value: unknown): void {
    void value;
  },
};

export function plainLogs(message: string): void {
  console.log("hello");
  console.info(message);
  logger.info({ event: "ready" });
}
