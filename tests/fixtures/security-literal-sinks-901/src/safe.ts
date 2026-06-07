import * as crypto from "node:crypto";
import * as fs from "node:fs";
import axios from "axios";
import { BrowserWindow } from "electron";
import mysql from "mysql";

declare const key: Buffer;
declare const iv: Buffer;
declare const file: string;
declare const token: string;

export function safeForms(): void {
  void fetch("https://api.example.com/status");
  void axios.get("sftp://files.example.com/report.csv");
  const socket = new WebSocket("wss://socket.example.com/events");
  socket.close();
  crypto.createCipheriv("aes-256-gcm", key, iv);
  fs.chmodSync(file, 0o644);
  fs.writeFileSync("tmp/fallow-token", token);
  new BrowserWindow({
    webPreferences: {
      nodeIntegration: false,
      webSecurity: true,
      contextIsolation: true,
    },
  });
  mysql.createConnection({ host: "localhost", multipleStatements: false });
}
