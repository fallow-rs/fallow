export interface ButtonProps {
  label: string;
}

export interface LegacyProps {
  text: string;
  color: string;
}

export const renderButton = (props: ButtonProps): string => `<button>${props.label}</button>`;
