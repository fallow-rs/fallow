import { useState, useEffect, useMemo, useCallback } from "react";

// A deliberately complex React component that exceeds the default cognitive /
// cyclomatic thresholds so it produces a complexity finding, and exercises a
// mix of hook kinds: several useState, several useEffect (one with a
// multi-element literal dependency array), useMemo, and useCallback. The
// per-component hook profile derived at the health layer reports the per-kind
// breakdown and the max useEffect dependency-array arity.
export const Dashboard = ({
  userId,
  region,
  locale,
  flags,
}: {
  userId: string;
  region: string;
  locale: string;
  flags: Record<string, boolean>;
}) => {
  const [count, setCount] = useState(0);
  const [name, setName] = useState("");
  const [items, setItems] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (userId) {
      setName(userId.toUpperCase());
    }
  }, [userId, region, locale]);

  useEffect(() => {
    if (count > 10) {
      setError("too many");
    } else if (count < 0) {
      setError("negative");
    } else {
      setError(null);
    }
  }, [count]);

  useEffect(() => {
    setItems([]);
  });

  const total = useMemo(() => {
    let acc = 0;
    for (const item of items) {
      if (item.length > 3) {
        acc += item.length;
      } else if (item.length === 0) {
        acc -= 1;
      } else {
        acc += 1;
      }
    }
    return acc;
  }, [items]);

  const handleClick = useCallback(() => {
    if (count > 5 && name) {
      setCount(count + 1);
    } else if (count <= 5 || !name) {
      setCount(count - 1);
    }
  }, [count, name]);

  let label = "idle";
  if (error) {
    label = "error";
  } else if (count > 100) {
    label = "max";
  } else if (count > 50) {
    label = "high";
  } else if (count > 20) {
    label = "medium";
  } else if (count > 5) {
    label = "low";
  } else if (total > 0) {
    label = "active";
  } else if (flags.beta) {
    label = "beta";
  } else if (region === "eu") {
    label = "eu";
  } else if (locale === "nl") {
    label = "nl";
  }

  return (
    <div>
      <span>{label}</span>
      <button onClick={handleClick}>{count}</button>
      <span>{name}</span>
      <span>{total}</span>
      {error ? <em>{error}</em> : null}
      <ul>
        {items.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
    </div>
  );
};
