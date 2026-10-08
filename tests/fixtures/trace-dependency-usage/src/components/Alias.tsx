import { Provider, useStore } from "react-redux";
import { store } from "../store/store";

const getStore = useStore;
export const Root = () => <Provider store={store}>{String(getStore())}</Provider>;
