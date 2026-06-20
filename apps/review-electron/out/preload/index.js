"use strict";
const electron = require("electron");
const api = {
  getReview: (root) => electron.ipcRenderer.invoke("review:get", root)
};
electron.contextBridge.exposeInMainWorld("fallow", api);
