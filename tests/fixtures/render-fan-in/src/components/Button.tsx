// A shared component rendered in MANY sites: the high-render-fan-in /
// blast-radius amplifier. Expected render-SITES = 6, distinct-parents = 3.
// Deliberately branchy so it also surfaces as a complexity finding, exercising
// the descriptive `blast radius: <Button> rendered in N places` drill-down line
// on the human hotspot/complexity surface.
export const Button = (props: {
  label: string;
  size?: "sm" | "md" | "lg";
  variant?: "primary" | "secondary" | "ghost";
  disabled?: boolean;
  loading?: boolean;
}) => {
  let cls = "btn";
  if (props.size === "sm") {
    cls += " btn-sm";
  } else if (props.size === "lg") {
    cls += " btn-lg";
  } else {
    cls += " btn-md";
  }
  if (props.variant === "primary") {
    cls += " btn-primary";
  } else if (props.variant === "secondary") {
    cls += " btn-secondary";
  } else if (props.variant === "ghost") {
    cls += " btn-ghost";
  }
  if (props.disabled) {
    cls += " is-disabled";
  }
  if (props.loading) {
    cls += " is-loading";
  }
  const text = props.loading ? "..." : props.label;
  return (
    <button className={cls} disabled={props.disabled || props.loading}>
      {text}
    </button>
  );
};
