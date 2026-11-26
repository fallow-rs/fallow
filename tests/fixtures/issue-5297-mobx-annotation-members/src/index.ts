import { AutoStore } from "./auto-store";
import { FilterStore } from "./filter-store";
import { LocalHelperStore } from "./local-helper";

AutoStore.configure();
export const stores = [new FilterStore(), new AutoStore(), new LocalHelperStore()];
