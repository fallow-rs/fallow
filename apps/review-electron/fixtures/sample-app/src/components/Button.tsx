type ButtonProps = {
  label: string;
  onClick: () => void;
  variant?: "primary" | "secondary";
};

export const Button = ({ label, onClick, variant = "primary" }: ButtonProps) => {
  const bg = variant === "primary" ? "#3b5bdb" : "#868e96";
  return (
    <button
      onClick={onClick}
      style={{
        background: bg,
        color: "white",
        border: "none",
        borderRadius: 6,
        padding: "8px 14px",
        marginRight: 8,
        cursor: "pointer",
      }}
    >
      {label}
    </button>
  );
};
