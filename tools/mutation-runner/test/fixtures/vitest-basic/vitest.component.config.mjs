/**
 * A small component compiler in the shape of the Vue and Svelte plugins. Its
 * `transform` hook compiles the whole component file and keeps the style
 * block. Its `load` hook serves the style block as a separate request with a
 * query, from the component that the `transform` hook compiled. The plugin
 * comes from the project config, so it runs before the overlay plugin of the
 * companion.
 */
const componentPlugin = () => {
  const styles = new Map();
  return {
    name: "fixture-component",
    enforce: "pre",
    load(id) {
      if (!id.endsWith(".vue?style")) return null;
      const style = styles.get(id.slice(0, -"?style".length)) ?? "not compiled";
      return `export default ${JSON.stringify(style)};`;
    },
    transform(code, id) {
      if (!id.endsWith(".vue")) return null;
      styles.set(id, /<style>([\s\S]*?)<\/style>/u.exec(code)?.[1].trim() ?? "");
      const script = /<script>([\s\S]*?)<\/script>/u.exec(code)?.[1] ?? "";
      return `${script}\nexport { default as style } from ${JSON.stringify(`${id}?style`)};\n`;
    },
  };
};

export default {
  plugins: [componentPlugin()],
  test: {
    include: ["components/**/*.test.ts"],
  },
};
