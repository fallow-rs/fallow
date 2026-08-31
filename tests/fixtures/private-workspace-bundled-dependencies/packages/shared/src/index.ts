import { cloneDeep } from "lodash-es";

export const cloneConfig = (config: Record<string, string>) => cloneDeep(config);
