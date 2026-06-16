// `Exported` is part of the package's public contract: its `label` prop is
// declared and read nowhere, but it MUST NOT be flagged (consumers pass it).
export const Exported = ({ label }: { label: string }) => <div>exported</div>;
