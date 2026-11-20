import moment from "moment";

export const invoiceDate = (date: Date): string => moment(date).format("YYYY-MM-DD");
