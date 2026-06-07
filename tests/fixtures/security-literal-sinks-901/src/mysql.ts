import mysql from "mysql";
import { createConnection } from "mysql2";

export function connect(): void {
  mysql.createConnection({
    host: "localhost",
    multipleStatements: true,
  });

  createConnection({
    host: "localhost",
    multipleStatements: true,
  });
}
