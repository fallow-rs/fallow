import axios from "axios";

export async function send(url: string): Promise<void> {
  await axios.put(url);
}
