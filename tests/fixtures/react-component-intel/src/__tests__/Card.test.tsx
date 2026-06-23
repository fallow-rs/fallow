import { Card } from "../components/Card";

// A test file rendering <Card> many times. These render sites MUST be excluded
// from render_sites / distinct_parents / pass counts (the test-file exclusion).
export const renderManyCards = () => {
  return (
    <div>
      <Card title="t1" subtitle="s1" />
      <Card title="t2" subtitle="s2" />
      <Card title="t3" subtitle="s3" />
      <Card title="t4" subtitle="s4" />
      <Card title="t5" subtitle="s5" />
    </div>
  );
};
