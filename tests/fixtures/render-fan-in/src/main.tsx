import { Home } from "./pages/Home";
import { Settings } from "./pages/Settings";
import { Toolbar } from "./widgets/Toolbar";
import { Dynamic } from "./pages/Dynamic";
import { Unrendered } from "./components/Unrendered";

// Keep Unrendered's module reachable WITHOUT rendering it as JSX, so the
// component is a real 0 in the render-fan-in population (rendered nowhere) rather
// than dropped as an unreachable file.
export const registry = { Unrendered };

// Entry point: renders every page/widget so they are all reachable.
export const App = () => {
  return (
    <main>
      <Home />
      <Settings />
      <Toolbar />
      <Dynamic />
    </main>
  );
};
