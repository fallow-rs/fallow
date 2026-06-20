import type { ReactNode } from "react";

type CardProps = {
  title: string;
  children: ReactNode;
};

export const Card = ({ title, children }: CardProps) => (
  <section
    style={{
      border: "1px solid #dee2e6",
      borderRadius: 10,
      padding: 16,
      maxWidth: 360,
    }}
  >
    <h2 style={{ marginTop: 0, fontSize: 18 }}>{title}</h2>
    {children}
  </section>
);
