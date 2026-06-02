const ALLOWED_URLS = ["https://api.example.com/users"];

export async function load(userUrl: string): Promise<Response> {
  if (!ALLOWED_URLS.includes(userUrl)) {
    throw new Error("url not allowed");
  }

  return fetch(userUrl);
}
