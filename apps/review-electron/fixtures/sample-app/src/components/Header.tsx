type HeaderProps = {
  title: string;
};

export const Header = ({ title }: HeaderProps) => (
  <header style={{ marginBottom: 20 }}>
    <h1 style={{ fontSize: 24, margin: 0 }}>{title}</h1>
  </header>
);
