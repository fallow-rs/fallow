import { slice, store } from "./store";

store.dispatch(slice.actions.inc());
export const value = store.getState();
