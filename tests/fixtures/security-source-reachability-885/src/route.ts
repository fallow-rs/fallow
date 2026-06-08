import { runUserCommand } from "./runner";

type RequestLike = {
  body: {
    command: string;
  };
};

export function handler(req: RequestLike): void {
  runUserCommand(req.body.command);
}
