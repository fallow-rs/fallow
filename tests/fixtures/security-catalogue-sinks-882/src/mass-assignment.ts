// Positive: Object.assign is flagged only when its source object traces to request input.
interface RequestLike {
  body: {
    profile: Record<string, unknown>;
  };
}

export function updateUser(target: Record<string, unknown>, req: RequestLike): void {
  const { profile } = req.body;
  Object.assign(target, profile);
}
