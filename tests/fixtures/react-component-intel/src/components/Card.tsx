import { useState, useEffect } from "react";

// Two props: `title` is read in the body, `subtitle` is declared but never read.
// Two hooks: one useState, one useEffect. Rendered 3 times from one parent
// (Home), so render_sites = 3, distinct_parents = 1.
export const Card = ({
  title,
  subtitle,
}: {
  title: string;
  subtitle: string;
}) => {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    setOpen(true);
  }, []);
  return <div className="card">{open ? title : ""}</div>;
};
