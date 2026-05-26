// Launched by the nested apps/web/playwright.config.ts webServer.command.
// Reachable only because the plugin resolves the path under the config directory.
const port = Number(process.env.PORT ?? 4000);

console.log(`nested e2e server listening on ${port}`);
