const knex = require("knex")({ client: "pg" });

function cjsSave(value) {
  if (value < 0) return 0;
  return knex("records").insert({ value });
}

module.exports = { cjsSave };
