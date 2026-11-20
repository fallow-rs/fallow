export const createResetLink = (userId: string): string => {
  const resetToken = Math.random().toString(36);
  return `/users/${userId}/reset?token=${resetToken}`;
};
