import { Manager } from "./manager.js";
import { SelectTool } from "./select-tool.js";

export function press(): void {
  const manager = new Manager();
  manager.register(new SelectTool());
  manager.current()?.onDown?.();
  manager.widget().paint();
}
