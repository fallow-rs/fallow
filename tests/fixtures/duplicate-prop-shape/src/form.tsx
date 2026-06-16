// GENUINE GROUP (member 3 of 3): the third component sharing the identical
// significant prop set { label, value, error, helpText }, in a SECOND file. This
// is what lifts the group past the >= 2-distinct-files floor.
export const FieldTextarea = ({ label, value, error, helpText, name, onChange }: {
  label: string;
  value: string;
  error: string;
  helpText: string;
  name: string;
  onChange: (v: string) => void;
}) => (
  <label>
    {label}
    <textarea value={value} name={name} onChange={(e) => onChange(e.target.value)} />
    <span>{error}</span>
    <small>{helpText}</small>
  </label>
);

// NOISE: two layout wrappers that share ONLY ubiquitous DOM props
// ({ className, style, children, onClick }). After the denylist the significant
// set is EMPTY, so they never group regardless of how many wrappers exist.
export const Box = ({ className, style, children, onClick }: {
  className: string;
  style: object;
  children: unknown;
  onClick: () => void;
}) => (
  <div className={className} style={style} onClick={onClick}>
    {children as never}
  </div>
);

export const Stack = ({ className, style, children, onClick }: {
  className: string;
  style: object;
  children: unknown;
  onClick: () => void;
}) => (
  <div className={className} style={style} onClick={onClick}>
    {children as never}
  </div>
);

// NOISE: a TWO-member shape that is below the rule-of-three group floor. Two
// cards sharing { title, subtitle, href, imageUrl } is tolerable duplication,
// not yet the abstraction trigger.
export const CardA = ({ title, subtitle, href, imageUrl }: {
  title: string;
  subtitle: string;
  href: string;
  imageUrl: string;
}) => (
  <a href={href}>
    <img src={imageUrl} />
    <h3>{title}</h3>
    <p>{subtitle}</p>
  </a>
);

export const CardB = ({ title, subtitle, href, imageUrl }: {
  title: string;
  subtitle: string;
  href: string;
  imageUrl: string;
}) => (
  <a href={href}>
    <img src={imageUrl} />
    <h3>{title}</h3>
    <p>{subtitle}</p>
  </a>
);
