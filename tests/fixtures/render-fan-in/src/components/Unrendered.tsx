// Exported but rendered NOWHERE (render fan-in 0). Confirms a real 0 is
// representable and does not panic the percentile (it stays in the population).
export const Unrendered = () => {
  return <section>never rendered as JSX</section>;
};
