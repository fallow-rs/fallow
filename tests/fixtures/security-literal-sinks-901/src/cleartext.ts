import axios from "axios";

export function connect(): void {
  void fetch("http://api.example.com/status");
  void axios.get("ftp://files.example.com/report.csv");
  const socket = new WebSocket("ws://socket.example.com/events");
  socket.close();
}
