const createHandler = () => (invoiceId: string): string => `sent ${invoiceId}`;

export const handler = createHandler();
export default handler;
