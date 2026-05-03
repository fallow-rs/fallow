import { describe, expect, it } from 'vitest';
import { mockS3Send } from '@aws-sdk/__mocks__';

describe('upload', () => {
  it('uses the manual mock', () => {
    expect(mockS3Send).toBeDefined();
  });
});
