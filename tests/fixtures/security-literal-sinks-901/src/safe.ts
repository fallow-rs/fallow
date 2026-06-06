import * as crypto from "node:crypto";
import axios from "axios";

declare const key: Buffer;
declare const iv: Buffer;

export function safeForms(): void {
  void fetch("https://api.example.com/status");
  void axios.get("sftp://files.example.com/report.csv");
  const socket = new WebSocket("wss://socket.example.com/events");
  socket.close();
  crypto.createCipheriv("aes-256-gcm", key, iv);
}
