export function GET(): Response {
  return new Response("[]");
}

export function POST(): Response {
  return new Response(null, { status: 201 });
}

export const pageSize = 20;
