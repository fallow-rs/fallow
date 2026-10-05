let tool: typeof import("./tool");

export function early(): string {
  return tool.earlyTool();
}

export async function setup(): Promise<void> {
  tool = await import("./tool");
}

export function run(): string {
  return tool.usedTool();
}
