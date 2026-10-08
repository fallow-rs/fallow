import { DataSource, type DataSourceOptions } from 'typeorm';

const isJest = process.argv.some((arg) => arg.includes('jest'));

export const coreOptions = {
  type: 'postgres',
  url: process.env.DATABASE_URL,
  entities: [`${isJest ? 'src/' : 'dist/'}engine/**/*.entity{.ts,.js}`],
  subscribers: 'src/events/*.subscriber.ts',
  migrations:
    process.env.IS_BILLING_ENABLED === 'true'
      ? [
          `${isJest ? 'src/' : 'dist/'}database/legacy/common/*{.ts,.js}`,
          `${isJest ? 'src/' : 'dist/'}database/legacy/billing/*{.ts,.js}`,
          `${process.env.LEGACY_ROOT}/legacy-schema/*.ts`,
        ]
      : [`${isJest ? 'src/' : 'dist/'}database/legacy/common/*{.ts,.js}`],
};

export const connectionSource = new DataSource(coreOptions as DataSourceOptions);
