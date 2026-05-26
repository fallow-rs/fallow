import http from "k6/http";

export default function scripted() {
  http.get("https://example.com");
}
