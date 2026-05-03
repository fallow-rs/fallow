import { describe, expect, it } from 'vitest';
import { mockS3Send } from '@aws-sdk/__mocks__';
import { mockSend } from '@supabase/__mocks__';
import { mockCapture } from '@sentry/__mocks__';

describe('upload', () => {
  it('uses scoped manual mocks', () => {
    expect(mockS3Send).toBeDefined();
    expect(mockSend).toBeDefined();
    expect(mockCapture).toBeDefined();
  });
});
