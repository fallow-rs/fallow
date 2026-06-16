import { mergeTests, test as base } from "@playwright/test";
import { MergedReader, SingleReader } from "./reader";

type SingleFixtures = {
  readerA: SingleReader;
  readerB: SingleReader;
};

export const testSingle = base.extend<SingleFixtures>({
  readerA: async ({}, use) => {
    await use(new SingleReader());
  },
  readerB: async ({}, use) => {
    await use(new SingleReader());
  },
});

type MergedReaderAFixture = {
  readerA: MergedReader;
};

type MergedReaderBFixture = {
  readerB: MergedReader;
};

const testMergedReaderA = base.extend<MergedReaderAFixture>({
  readerA: async ({}, use) => {
    await use(new MergedReader());
  },
});

const testMergedReaderB = base.extend<MergedReaderBFixture>({
  readerB: async ({}, use) => {
    await use(new MergedReader());
  },
});

export const testMerged = mergeTests(testMergedReaderA, testMergedReaderB);
