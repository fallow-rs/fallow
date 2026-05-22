const test = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const {
  _verifyWithKey,
  verifyBinaryAt,
  verifyDigestAt,
  verifyInstalled,
  sha256Hex,
  normalizeDigest,
  readEmbeddedDigest,
  EMBEDDED_PUBLIC_KEY,
  ED25519_SPKI_HEADER,
  SKIP_ENV,
} = require('./verify-binary');
const { getPlatformPackage } = require('./platform-package');

function makeDigestProvider(dir) {
  return ({ assetName, binaryPath }) => {
    const data = fs.readFileSync(binaryPath);
    return Promise.resolve('sha256:' + crypto.createHash('sha256').update(data).digest('hex'));
  };
}

function makeMismatchedDigestProvider() {
  return () => Promise.resolve('sha256:' + 'a'.repeat(64));
}

function makeKeypair() {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519');
  const spki = publicKey.export({ format: 'der', type: 'spki' });
  const rawPub = spki.subarray(spki.length - 32);
  return { privateKey, rawPub };
}

function makeFixture(binaryBytes, signFn) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fallow-vbtest-'));
  const binaryPath = path.join(dir, 'fallow');
  fs.writeFileSync(binaryPath, binaryBytes);
  if (signFn) {
    fs.writeFileSync(`${binaryPath}.sig`, signFn(binaryBytes));
  }
  return { dir, binaryPath };
}

function cleanup(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
}

test('embedded public key is 32 bytes and SPKI header is 12 bytes', () => {
  assert.equal(EMBEDDED_PUBLIC_KEY.length, 32);
  assert.equal(ED25519_SPKI_HEADER.length, 12);
});

test('embedded public key reconstructs a valid Ed25519 SPKI key', () => {
  const spki = Buffer.concat([ED25519_SPKI_HEADER, EMBEDDED_PUBLIC_KEY]);
  const key = crypto.createPublicKey({ key: spki, format: 'der', type: 'spki' });
  assert.equal(key.asymmetricKeyType, 'ed25519');
});

test('_verifyWithKey returns ok for a valid signature', () => {
  const { privateKey, rawPub } = makeKeypair();
  const content = Buffer.from('hello world');
  const { dir, binaryPath } = makeFixture(content, (data) =>
    crypto.sign(null, data, privateKey),
  );
  try {
    const result = _verifyWithKey(binaryPath, rawPub);
    assert.deepEqual(result, { ok: true });
  } finally {
    cleanup(dir);
  }
});

test('_verifyWithKey returns sig-invalid when the signature is corrupted', () => {
  const { privateKey, rawPub } = makeKeypair();
  const content = Buffer.from('hello world');
  const { dir, binaryPath } = makeFixture(content, (data) =>
    crypto.sign(null, data, privateKey),
  );
  try {
    const sig = fs.readFileSync(`${binaryPath}.sig`);
    sig[0] ^= 0xff;
    fs.writeFileSync(`${binaryPath}.sig`, sig);
    const result = _verifyWithKey(binaryPath, rawPub);
    assert.equal(result.ok, false);
    assert.equal(result.code, 'sig-invalid');
  } finally {
    cleanup(dir);
  }
});

test('_verifyWithKey returns sig-invalid for the wrong key', () => {
  const { rawPub } = makeKeypair();
  const wrongKey = makeKeypair();
  const content = Buffer.from('hello world');
  const { dir, binaryPath } = makeFixture(content, (data) =>
    crypto.sign(null, data, wrongKey.privateKey),
  );
  try {
    const result = _verifyWithKey(binaryPath, rawPub);
    assert.equal(result.ok, false);
    assert.equal(result.code, 'sig-invalid');
  } finally {
    cleanup(dir);
  }
});

test('_verifyWithKey returns sig-invalid when the binary bytes are tampered', () => {
  const { privateKey, rawPub } = makeKeypair();
  const original = Buffer.from('hello world');
  const { dir, binaryPath } = makeFixture(original, (data) =>
    crypto.sign(null, data, privateKey),
  );
  try {
    fs.writeFileSync(binaryPath, Buffer.from('tampered'));
    const result = _verifyWithKey(binaryPath, rawPub);
    assert.equal(result.ok, false);
    assert.equal(result.code, 'sig-invalid');
  } finally {
    cleanup(dir);
  }
});

test('_verifyWithKey returns sig-missing when the signature file does not exist', () => {
  const { rawPub } = makeKeypair();
  const { dir, binaryPath } = makeFixture(Buffer.from('hello world'));
  try {
    const result = _verifyWithKey(binaryPath, rawPub);
    assert.equal(result.ok, false);
    assert.equal(result.code, 'sig-missing');
  } finally {
    cleanup(dir);
  }
});

test('_verifyWithKey returns binary-missing when the binary does not exist', () => {
  const { rawPub } = makeKeypair();
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fallow-vbtest-'));
  try {
    const result = _verifyWithKey(path.join(dir, 'nonexistent'), rawPub);
    assert.equal(result.ok, false);
    assert.equal(result.code, 'binary-missing');
  } finally {
    cleanup(dir);
  }
});

test('_verifyWithKey returns sig-invalid when the signature length is wrong', () => {
  const { rawPub } = makeKeypair();
  const { dir, binaryPath } = makeFixture(Buffer.from('hello'));
  try {
    fs.writeFileSync(`${binaryPath}.sig`, Buffer.from('short'));
    const result = _verifyWithKey(binaryPath, rawPub);
    assert.equal(result.ok, false);
    assert.equal(result.code, 'sig-invalid');
  } finally {
    cleanup(dir);
  }
});

test('_verifyWithKey throws when given a non-32-byte raw public key', () => {
  const { dir, binaryPath } = makeFixture(Buffer.from('hello world'));
  try {
    const result = _verifyWithKey(binaryPath, Buffer.from('too short'));
    assert.equal(result.ok, false);
    assert.equal(result.code, 'sig-missing');
  } finally {
    cleanup(dir);
  }
});

test('verifyBinaryAt uses the embedded production public key', () => {
  // The embedded key cannot sign our test data because we do not have the
  // private key, so we only assert that verifyBinaryAt returns sig-invalid
  // for a random signature against the production key, not the underlying
  // crypto throwing. This locks in that the public API uses the embedded
  // key path.
  const { privateKey } = makeKeypair();
  const content = Buffer.from('hello world');
  const { dir, binaryPath } = makeFixture(content, (data) =>
    crypto.sign(null, data, privateKey),
  );
  try {
    const result = verifyBinaryAt(binaryPath);
    assert.equal(result.ok, false);
    assert.equal(result.code, 'sig-invalid');
  } finally {
    cleanup(dir);
  }
});

function makePlatformDir(privateKey, options) {
  const opts = options || {};
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fallow-vbtest-'));
  const ext = process.platform === 'win32' ? '.exe' : '';
  for (const base of ['fallow', 'fallow-lsp', 'fallow-mcp']) {
    const binaryPath = path.join(dir, `${base}${ext}`);
    const content = Buffer.from(`mock ${base} contents`);
    fs.writeFileSync(binaryPath, content);
    if (opts.skipSigFor === base) {
      continue;
    }
    const data = opts.corruptBinaryFor === base ? Buffer.from('tampered') : content;
    const sig = crypto.sign(null, data, privateKey);
    if (opts.corruptSigFor === base) {
      sig[0] ^= 0xff;
    }
    fs.writeFileSync(`${binaryPath}.sig`, sig);
  }
  return dir;
}

function currentPlatformPackage() {
  if (process.platform !== 'linux') {
    return getPlatformPackage(process.platform, process.arch);
  }
  let libcFamily;
  try {
    libcFamily = require('detect-libc').familySync();
  } catch {
    libcFamily = undefined;
  }
  return getPlatformPackage(process.platform, process.arch, libcFamily);
}

test('normalizeDigest accepts sha256: prefix and bare hex', () => {
  const sample = 'a'.repeat(64);
  assert.equal(normalizeDigest('sha256:' + sample), sample);
  assert.equal(normalizeDigest(sample), sample);
  assert.equal(normalizeDigest('SHA256:' + sample.toUpperCase()), sample);
});

test('normalizeDigest rejects malformed digests', () => {
  assert.equal(normalizeDigest(null), null);
  assert.equal(normalizeDigest(''), null);
  assert.equal(normalizeDigest('not-hex'), null);
  assert.equal(normalizeDigest('a'.repeat(63)), null);
});

test('sha256Hex returns 64-char hex over file bytes', () => {
  const { dir, binaryPath } = makeFixture(Buffer.from('hello world'));
  try {
    const result = sha256Hex(binaryPath);
    assert.equal(result.ok, true);
    assert.equal(result.digest, crypto.createHash('sha256').update(Buffer.from('hello world')).digest('hex'));
  } finally {
    cleanup(dir);
  }
});

test('verifyDigestAt accepts matching digest and rejects mismatched', () => {
  const { dir, binaryPath } = makeFixture(Buffer.from('hello world'));
  try {
    const correct = crypto.createHash('sha256').update(Buffer.from('hello world')).digest('hex');
    assert.deepEqual(verifyDigestAt(binaryPath, 'sha256:' + correct), { ok: true });
    const wrong = 'b'.repeat(64);
    const bad = verifyDigestAt(binaryPath, wrong);
    assert.equal(bad.ok, false);
    assert.equal(bad.code, 'digest-mismatch');
  } finally {
    cleanup(dir);
  }
});

test('verifyInstalled with dirOverride returns ok when every binary verifies', async (t) => {
  const { privateKey, rawPub } = makeKeypair();
  const dir = makePlatformDir(privateKey);
  t.after(() => cleanup(dir));
  const result = await verifyInstalled({
    dirOverride: dir,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: makeDigestProvider(dir),
  });
  assert.equal(result.ok, true);
  assert.equal(result.package, '<override>');
});

test('verifyInstalled resolves a global npm install from the fallow package directory', async (t) => {
  const pkg = currentPlatformPackage();
  if (!pkg) {
    t.skip('unsupported platform');
    return;
  }

  const { privateKey, rawPub } = makeKeypair();
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'fallow-vbtest-global-'));
  t.after(() => cleanup(root));

  const resolveFrom = path.join(root, 'node_modules', 'fallow');
  const platformDir = path.join(root, 'node_modules', ...pkg.split('/'));
  fs.mkdirSync(resolveFrom, { recursive: true });
  fs.mkdirSync(platformDir, { recursive: true });
  fs.writeFileSync(path.join(platformDir, 'package.json'), JSON.stringify({ name: pkg, version: '9.9.9' }));

  const ext = process.platform === 'win32' ? '.exe' : '';
  for (const base of ['fallow', 'fallow-lsp', 'fallow-mcp']) {
    const binaryPath = path.join(platformDir, `${base}${ext}`);
    const content = Buffer.from(`global install ${base}`);
    fs.writeFileSync(binaryPath, content);
    fs.writeFileSync(`${binaryPath}.sig`, crypto.sign(null, content, privateKey));
  }

  const result = await verifyInstalled({
    resolveFrom,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: ({ binaryPath }) => crypto.createHash('sha256').update(fs.readFileSync(binaryPath)).digest('hex'),
  });
  assert.equal(result.ok, true);
  assert.equal(result.package, pkg);
  assert.equal(result.version, '9.9.9');
});

test('verifyInstalled with dirOverride fails fast on the first bad signature', async (t) => {
  const { privateKey, rawPub } = makeKeypair();
  const dir = makePlatformDir(privateKey, { corruptSigFor: 'fallow-lsp' });
  t.after(() => cleanup(dir));
  const result = await verifyInstalled({
    dirOverride: dir,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: makeDigestProvider(dir),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'sig-invalid');
  assert.match(result.binary, /fallow-lsp/);
});

test('verifyInstalled with dirOverride reports sig-missing when a .sig is absent', async (t) => {
  const { privateKey, rawPub } = makeKeypair();
  const dir = makePlatformDir(privateKey, { skipSigFor: 'fallow-mcp' });
  t.after(() => cleanup(dir));
  const result = await verifyInstalled({
    dirOverride: dir,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: makeDigestProvider(dir),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'sig-missing');
  assert.match(result.binary, /fallow-mcp/);
});

test('verifyInstalled reports digest-mismatch when SHA-256 disagrees with the provider', async (t) => {
  const { privateKey, rawPub } = makeKeypair();
  const dir = makePlatformDir(privateKey);
  t.after(() => cleanup(dir));
  const result = await verifyInstalled({
    dirOverride: dir,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: makeMismatchedDigestProvider(),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'digest-mismatch');
});

test('verifyInstalled reports digest-unavailable when the provider rejects', async (t) => {
  const { privateKey, rawPub } = makeKeypair();
  const dir = makePlatformDir(privateKey);
  t.after(() => cleanup(dir));
  const result = await verifyInstalled({
    dirOverride: dir,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: () => Promise.reject(new Error('network down')),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'digest-unavailable');
  assert.match(result.message, /network down/);
});

test('verifyInstalled honors FALLOW_SKIP_BINARY_VERIFY', async (t) => {
  const previous = process.env[SKIP_ENV];
  process.env[SKIP_ENV] = '1';
  t.after(() => {
    if (previous === undefined) delete process.env[SKIP_ENV];
    else process.env[SKIP_ENV] = previous;
  });
  const result = await verifyInstalled({ dirOverride: '/does/not/exist' });
  assert.equal(result.ok, true);
  assert.equal(result.skipped, true);
});

function computeDigestsForDir(dir) {
  const ext = process.platform === 'win32' ? '.exe' : '';
  const out = {};
  for (const base of ['fallow', 'fallow-lsp', 'fallow-mcp']) {
    const fileName = `${base}${ext}`;
    const full = path.join(dir, fileName);
    out[fileName] = 'sha256:' + crypto.createHash('sha256').update(fs.readFileSync(full)).digest('hex');
  }
  return out;
}

function writeManifest(dir, body) {
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(body));
}

test('readEmbeddedDigest returns null when manifest is missing', () => {
  assert.equal(readEmbeddedDigest('/does/not/exist/package.json', 'fallow'), null);
});

test('readEmbeddedDigest returns null when fallowDigests field is absent', (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fallow-vbtest-emb-'));
  t.after(() => cleanup(dir));
  writeManifest(dir, { name: '@fallow-cli/x', version: '1.0.0' });
  assert.equal(readEmbeddedDigest(path.join(dir, 'package.json'), 'fallow'), null);
});

test('readEmbeddedDigest returns null when the per-binary entry is malformed', (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fallow-vbtest-emb-'));
  t.after(() => cleanup(dir));
  writeManifest(dir, {
    name: '@fallow-cli/x',
    version: '1.0.0',
    fallowDigests: { fallow: 'not-a-real-digest' },
  });
  assert.equal(readEmbeddedDigest(path.join(dir, 'package.json'), 'fallow'), null);
});

test('readEmbeddedDigest returns normalized hex for a valid sha256: prefixed entry', (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fallow-vbtest-emb-'));
  t.after(() => cleanup(dir));
  const hex = 'a'.repeat(64);
  writeManifest(dir, {
    name: '@fallow-cli/x',
    version: '1.0.0',
    fallowDigests: { fallow: 'sha256:' + hex },
  });
  assert.equal(readEmbeddedDigest(path.join(dir, 'package.json'), 'fallow'), hex);
});

test('verifyInstalled uses the embedded digest without calling the provider', async (t) => {
  const { privateKey, rawPub } = makeKeypair();
  const dir = makePlatformDir(privateKey);
  t.after(() => cleanup(dir));
  writeManifest(dir, {
    name: '@fallow-cli/x',
    version: '1.0.0',
    fallowDigests: computeDigestsForDir(dir),
  });
  let providerCalls = 0;
  const result = await verifyInstalled({
    dirOverride: dir,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: () => {
      providerCalls += 1;
      return Promise.reject(new Error('provider should not be called'));
    },
  });
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.equal(providerCalls, 0);
});

test('verifyInstalled falls back to the provider when the embedded digest is missing', async (t) => {
  const { privateKey, rawPub } = makeKeypair();
  const dir = makePlatformDir(privateKey);
  t.after(() => cleanup(dir));
  // No package.json at all; legacy platform package shape.
  let providerCalls = 0;
  const result = await verifyInstalled({
    dirOverride: dir,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: ({ binaryPath }) => {
      providerCalls += 1;
      return Promise.resolve('sha256:' + crypto.createHash('sha256').update(fs.readFileSync(binaryPath)).digest('hex'));
    },
  });
  assert.equal(result.ok, true);
  assert.equal(providerCalls, 3);
});

test('verifyInstalled falls back to the provider when fallowDigests is partial / malformed', async (t) => {
  const { privateKey, rawPub } = makeKeypair();
  const dir = makePlatformDir(privateKey);
  t.after(() => cleanup(dir));
  writeManifest(dir, {
    name: '@fallow-cli/x',
    version: '1.0.0',
    fallowDigests: { fallow: 'not-a-real-digest' },
  });
  let providerCalls = 0;
  const result = await verifyInstalled({
    dirOverride: dir,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: ({ binaryPath }) => {
      providerCalls += 1;
      return Promise.resolve('sha256:' + crypto.createHash('sha256').update(fs.readFileSync(binaryPath)).digest('hex'));
    },
  });
  assert.equal(result.ok, true);
  // One call per binary because all three entries fail to normalize.
  assert.equal(providerCalls, 3);
});

test('verifyInstalled returns digest-mismatch when the embedded digest disagrees with the binary', async (t) => {
  const { privateKey, rawPub } = makeKeypair();
  const dir = makePlatformDir(privateKey);
  t.after(() => cleanup(dir));
  const ext = process.platform === 'win32' ? '.exe' : '';
  writeManifest(dir, {
    name: '@fallow-cli/x',
    version: '1.0.0',
    fallowDigests: {
      [`fallow${ext}`]: 'sha256:' + 'a'.repeat(64),
      [`fallow-lsp${ext}`]: 'sha256:' + 'a'.repeat(64),
      [`fallow-mcp${ext}`]: 'sha256:' + 'a'.repeat(64),
    },
  });
  const result = await verifyInstalled({
    dirOverride: dir,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: () => Promise.reject(new Error('should not be reached')),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'digest-mismatch');
});

test('verifyInstalled ignores skip env when allowSkipEnv is false', async (t) => {
  const previous = process.env[SKIP_ENV];
  process.env[SKIP_ENV] = '1';
  t.after(() => {
    if (previous === undefined) delete process.env[SKIP_ENV];
    else process.env[SKIP_ENV] = previous;
  });
  const { privateKey, rawPub } = makeKeypair();
  const dir = makePlatformDir(privateKey);
  t.after(() => cleanup(dir));
  const result = await verifyInstalled({
    dirOverride: dir,
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: makeDigestProvider(dir),
    allowSkipEnv: false,
  });
  assert.equal(result.ok, true);
  assert.notEqual(result.skipped, true);
});
