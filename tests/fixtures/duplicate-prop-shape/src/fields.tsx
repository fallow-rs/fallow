// GENUINE GROUP (members 1 + 2 of 3): two field components in this file that
// declare an identical SIGNIFICANT prop set { label, value, error, helpText }.
// `name` and `onChange` are ubiquitous-denylist names and do not count toward
// the significant set, but the four remaining domain props are byte-identical
// across every member, and a third member lives in form.tsx. Group size 3,
// across 2 files: a real missing shared `FieldProps` abstraction.

export const FieldText = ({ label, value, error, helpText, name, onChange }: {
  label: string;
  value: string;
  error: string;
  helpText: string;
  name: string;
  onChange: (v: string) => void;
}) => (
  <label>
    {label}
    <input value={value} name={name} onChange={(e) => onChange(e.target.value)} />
    <span>{error}</span>
    <small>{helpText}</small>
  </label>
);

export const FieldNumber = ({ label, value, error, helpText, name, onChange }: {
  label: string;
  value: string;
  error: string;
  helpText: string;
  name: string;
  onChange: (v: string) => void;
}) => (
  <label>
    {label}
    <input type="number" value={value} name={name} onChange={(e) => onChange(e.target.value)} />
    <span>{error}</span>
    <small>{helpText}</small>
  </label>
);

// NOISE: a SUPERSET member. It has the four shared props PLUS a fifth
// (`options`), so its significant set is { label, value, error, helpText,
// options }: NOT byte-identical to the four-prop group. Exact full-set identity
// only, so this must NOT join the FieldText/FieldNumber group.
export const FieldSelectWithOptions = ({
  label,
  value,
  error,
  helpText,
  options,
  name,
  onChange,
}: {
  label: string;
  value: string;
  error: string;
  helpText: string;
  options: string[];
  name: string;
  onChange: (v: string) => void;
}) => (
  <label>
    {label}
    <select value={value} name={name} onChange={(e) => onChange(e.target.value)}>
      {options.map((o) => (
        <option key={o}>{o}</option>
      ))}
    </select>
    <span>{error}</span>
    <small>{helpText}</small>
  </label>
);
