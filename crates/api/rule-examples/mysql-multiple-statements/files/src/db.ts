import mysql from "mysql";

export const connection = mysql.createConnection({
  host: "localhost",
  user: "shop",
  multipleStatements: true,
});
