declare const server: {
  tool(name: string, schema: unknown, handler: (input: { city: unknown }) => void): void;
};

server.tool("lookup", {}, ({ city }) => {
  eval(city);
});
