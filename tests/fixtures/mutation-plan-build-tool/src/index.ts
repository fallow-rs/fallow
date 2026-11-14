import { stampText } from "../vite-plugins/stamp";

export const version = (): string => stampText(3);
