import { useState } from "react";
import { Header } from "./components/Header";
import { Card } from "./components/Card";
import { Button } from "./components/Button";

export const App = () => {
  const [count, setCount] = useState(0);
  return (
    <main style={{ fontFamily: "system-ui", padding: 24 }}>
      <Header title="Sample app under review" />
      <Card title="Counter">
        <p>Count: {count}</p>
        <Button label="Increment" onClick={() => setCount((c) => c + 1)} />
        <Button label="Reset" variant="secondary" onClick={() => setCount(0)} />
      </Card>
    </main>
  );
};
