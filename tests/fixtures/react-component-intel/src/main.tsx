import { Home } from "./pages/Home";
import { renderManyCards } from "./__tests__/Card.test";

// Keep the test module reachable without it being treated as production render
// blast radius (the test-file exclusion handles that). Entry point renders Home.
export const registry = { renderManyCards };

export const App = () => {
  return (
    <main>
      <Home />
    </main>
  );
};
