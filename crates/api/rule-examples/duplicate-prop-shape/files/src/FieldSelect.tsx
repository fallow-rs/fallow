type Props = {
  name: string;
  label: string;
  value: string;
  error: string;
  hint: string;
  onChange: (value: string) => void;
};

export const FieldSelect = ({ name, label, value, error, hint, onChange }: Props) => (
  <label>
    {label}
    <select name={name} value={value} onChange={(e) => onChange(e.target.value)} />
    <small>{hint}</small>
    <span>{error}</span>
  </label>
);
