/**
 * Only referenced from a JSDoc type annotation.
 *
 * @param cfg {import('./lib/types.ts').Config}
 */
function boot(cfg) {
  console.log('booting', cfg);
}

boot({ theme: 'dark' });
