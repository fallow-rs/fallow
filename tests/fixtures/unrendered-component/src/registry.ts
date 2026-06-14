import Lazy from "./components/Lazy.vue";

// `Lazy` is value-read here (a script-side use), never rendered as a tag. It must
// be credited as used (the liberal posture), so it is NOT flagged.
export const registry = {
  Lazy,
};
