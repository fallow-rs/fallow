// Launched by Playwright's webServer.command (`tsx scripts/e2e-server.ts`).
// Not imported anywhere; reachable only because the plugin seeds it as a setup file.
const port = Number(process.env.PORT ?? 4000);

console.log(`e2e server listening on ${port}`);
