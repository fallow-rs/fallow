export async function load(userUrl: string): Promise<Response> {
  if (!isAllowedUrl(userUrl)) {
    throw new Error("url not allowed");
  }

  return fetch(userUrl);
}

const isAllowedUrl = (url: string): boolean =>
  url === "https://api.example.com/users";
