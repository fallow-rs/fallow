import { chainTest } from "../src/chain-fixture";

chainTest("draws and exports a chart", async ({ chart, exporter }) => {
  chart.drawChart();
  exporter.runExport();
});
