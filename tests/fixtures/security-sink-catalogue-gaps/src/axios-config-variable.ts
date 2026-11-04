import axios from "axios";

export async function send(config: object): Promise<void> {
  await axios(config);
}
