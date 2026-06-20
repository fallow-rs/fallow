"use strict";
const electron = require("electron");
const api = {
  getReview: (root) => electron.ipcRenderer.invoke("review:get", root),
  getGuide: (root) => electron.ipcRenderer.invoke("review:guide", root),
  appendFeed: (item) => electron.ipcRenderer.invoke("feed:append", item),
  validate: (hash, items) => electron.ipcRenderer.invoke("review:validate", hash, items),
  capture: (url) => electron.ipcRenderer.invoke("shot:capture", url),
  saveShot: (payload) => electron.ipcRenderer.invoke("shot:save", payload)
};
electron.contextBridge.exposeInMainWorld("fallow", api);
