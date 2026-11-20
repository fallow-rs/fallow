type Props = {
  name: string;
  label: string;
  value: string;
  error: string;
  hint: string;
  onChange: (value: string) => void;
};

export const FieldText = ({ name, label, value, error, hint, onChange }: Props) => (
  <label>
    {label}
    <input type="text" name={name} value={value} onChange={(e) => onChange(e.target.value)} />
    <small>{hint}</small>
    <span>{error}</span>
  </label>
);
