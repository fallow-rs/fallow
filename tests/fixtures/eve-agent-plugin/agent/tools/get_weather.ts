import { formatTemperature } from "../lib/format";

export default {
  description: "Return the weather for a city.",
  execute: (city: string): string => `${city}: ${formatTemperature(21)}`,
};
