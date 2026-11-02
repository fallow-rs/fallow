const defineConfig = <T>(config: T): T => config;

export default defineConfig({
  schema: "./schema/github.graphql",
  documents: ["src/**/*.graphql", "src/**/*.ts", "../../shared/*.graphql"],
});
