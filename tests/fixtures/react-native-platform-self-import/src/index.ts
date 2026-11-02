import { StatusIcon, type StatusIconName } from "./components/StatusIcon";
import { Toolbar } from "./components/Toolbar";

const initial: StatusIconName = "check";

export const app = [StatusIcon(initial), Toolbar];
