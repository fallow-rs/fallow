export function describeEvent(event) {
  if (event.type === "test:pass") {
    return "pass";
  }
  if (event.type === "test:fail") {
    return "fail";
  }
  return event.type;
}
