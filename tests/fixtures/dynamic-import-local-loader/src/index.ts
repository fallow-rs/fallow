declare const lazyLoad: (load: () => Promise<unknown>) => unknown;

const loadView = () => import('./view');
export const View = lazyLoad(loadView);

export async function run(): Promise<void> {
  const helpers = await loadHelpers();
  helpers.usedHelper();
  const { usedTool } = await loadTools();
  usedTool();
}

async function loadHelpers() {
  return await import('./helpers');
}

function loadTools() {
  return import('./tools');
}

const loadPanel = () => import('./panel');
export const panelReady = loadPanel().then((panel) => panel);

const loadDrawer = () => import('./drawer');
