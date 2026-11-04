import axios from "axios";

export async function send(host: string): Promise<void> {
  await axios(`https://${host}/status`);
}
