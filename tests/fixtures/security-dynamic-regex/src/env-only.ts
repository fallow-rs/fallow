// Negative: a deployment secret is not attacker input, so a pattern from the
// environment alone is not a dynamic-regex candidate.
export const fromEnv = (): RegExp => new RegExp(process.env.ALLOWED_PATTERN ?? "^$");
