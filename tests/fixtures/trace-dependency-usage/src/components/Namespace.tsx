import * as RR from "react-redux";
import { store } from "../store/store";

export const App = () => (
  <RR.Provider store={store}>
    <span>{String(RR.useSelector((s: { count: number }) => s.count))}</span>
  </RR.Provider>
);
export const hooks = [RR.useDispatch];
