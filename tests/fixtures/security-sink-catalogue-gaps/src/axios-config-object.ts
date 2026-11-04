import axios from "axios";

export async function send(id: string): Promise<void> {
  await axios({ method: "post", url: "/api/items", data: { id } });
}
