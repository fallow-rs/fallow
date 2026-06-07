import mysql from "mysql";
import { createConnection } from "mysql2";
import mysql2 from "mysql2/promise";
import { createPool } from "mysql2/promise";

export function connect(): void {
  mysql.createConnection({
    host: "localhost",
    multipleStatements: true,
  });

  createConnection({
    host: "localhost",
    multipleStatements: true,
  });

  mysql2.createConnection({
    host: "localhost",
    multipleStatements: true,
  });

  createPool({
    host: "localhost",
    multipleStatements: true,
  });
}
