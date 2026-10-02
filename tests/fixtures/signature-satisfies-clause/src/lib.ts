export type Channel = "email" | "sms" | "push";

export const CHANNELS = ["email", "sms"] as const satisfies readonly Channel[];

type LocalLimits = Record<string, number>;

export const LIMITS = { daily: 10 } satisfies LocalLimits;

export interface EventPayload {
  id: string;
}

export type EventHandler = (payload: EventPayload) => void;

export const onEvent = ((payload: EventPayload): void => {
  console.log(payload.id);
}) satisfies EventHandler;

export interface HiddenShape {
  hidden: number;
}

const hidden = { hidden: 1 } satisfies HiddenShape;

console.log(hidden);
