import { test } from '@playwright/test';
import leftPad from 'left-pad';

test('pads', () => {
  leftPad('a', 2);
});
