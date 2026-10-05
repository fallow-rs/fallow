interface Props {
  title: string;
  highlight?: boolean;
}
export function Card({ title, highlight = false }: Props) {
  return <p className={highlight ? 'bright' : ''}>{title}</p>;
}
