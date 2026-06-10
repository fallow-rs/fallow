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

// Public-by-convention env vars are build-inlined, not secrets: logging one must
// NOT fire secret-pii-log (issue #890 GAP A). Without the public-prefix exclusion
// this `process.env` read would be treated as a secret source and fire.
export function logsPublicEnv(): void {
  console.log(process.env.NEXT_PUBLIC_API_URL);
  console.info(import.meta.env.VITE_BUILD_ID);
}

export function logsPublicCiMetadata(): void {
  const tagRef = process.env.TAG_REF;
  console.error(`unexpected TAG_REF: ${tagRef}`);
}
