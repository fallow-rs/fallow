import { defineConfig } from "sanity";
import { schemaTypes } from "./schemas";
import { StudioLogo } from "./src/logo";

export const unusedHelper = () => "helper";

export default defineConfig({
  name: "default",
  title: "Studio",
  projectId: "abc123",
  dataset: "production",
  icon: () => <StudioLogo />,
  schema: { types: schemaTypes },
});
