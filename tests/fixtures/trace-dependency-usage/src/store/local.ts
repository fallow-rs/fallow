import { useSelector as pick } from "react-redux";

const selectCount = pick.withTypes<{ count: number }>();
const typedPick = pick;
export { typedPick as usePick };
export const readCount = () => selectCount((s) => s.count);
export const maybe = () => pick?.((s: { count: number }) => s.count);
