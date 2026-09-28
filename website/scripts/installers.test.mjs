import assert from 'node:assert/strict';
import test from 'node:test';
import { createInstallers } from './installers.mjs';

test('release installers pin both platforms to the requested tag', async () => {
  const files = await createInstallers('v1.2.3-preview.1');
  assert.match(files.get('/install.sh'), /^version=v1\.2\.3-preview\.1$/m);
  assert.match(files.get('/install.ps1'), /\$Version = 'v1\.2\.3-preview\.1'/);
  assert.equal(files.size, 2);
});

test('release tags cannot inject shell or PowerShell code', async () => {
  for (const tag of ["v1.2.3'; exit", 'v1.2.3\necho injected', '../v1.2.3']) {
    await assert.rejects(createInstallers(tag), /Invalid release tag/);
  }
});
