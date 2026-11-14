export const readFlag = (): string => (import.meta.env.VITE_FLAG === "on" ? "on" : "off");
