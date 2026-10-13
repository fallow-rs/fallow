import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/")({
  loader: (): number => 1 + 1,
});
