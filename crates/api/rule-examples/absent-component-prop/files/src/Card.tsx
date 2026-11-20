interface CardProps {
  title: string;
  highlight?: boolean;
}

export function Card({ title, highlight = false }: CardProps) {
  return <h2 className={highlight ? "card card-highlight" : "card"}>{title}</h2>;
}
