import type { ToolbarProps } from "./Toolbar";

export const Toolbar = (props: ToolbarProps): string => `android:${props.title}`;
