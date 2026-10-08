import { connect, useSelector as useSel } from "react-redux";
import { useAppDispatch, useAppSelector } from "../store/hooks";

const Counter = () => {
  const count = useAppSelector((s) => s.count);
  const total = useAppSelector((s) => s.count * 2);
  const raw = useSel((s: { count: number }) => s.count);
  const dispatch = useAppDispatch();
  return <button onClick={() => dispatch({ type: "inc" })}>{count + total + raw}</button>;
};

const mapState = (s: { count: number }) => ({ count: s.count });
export default connect(mapState)(Counter);
