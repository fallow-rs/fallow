import { Button } from "../components/Button";

// A TEST-LOCAL component rendered MANY times in a test loop. On real repos this
// is exactly the noise that dominated the headline (TanStack: a test-local
// `Page` with render_sites=146 but distinct_parents=2). It must NOT appear as a
// fan-in target / top-N entry, and its render SITES must NOT count toward
// Button's fan-in (the parent file is a test file).
const Page = () => {
  return (
    <div>
      <Button label="t1" />
      <Button label="t2" />
      <Button label="t3" />
      <Button label="t4" />
      <Button label="t5" />
    </div>
  );
};

it("renders the page many times", () => {
  // Render <Page> a lot: high render-SITE count, but test-local noise.
  for (let i = 0; i < 50; i++) {
    Page();
  }
});
