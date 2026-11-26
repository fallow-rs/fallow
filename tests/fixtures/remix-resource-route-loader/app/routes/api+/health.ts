export const loader = async () => {
  return Response.json({ status: "ok", timestamp: Date.now(), checks: {} });
};
