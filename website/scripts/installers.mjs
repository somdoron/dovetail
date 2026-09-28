import { readFile } from 'node:fs/promises';

// Website endpoints install the compiler version from the deployed checkout.
export async function createInstallers(tag) {
  if (!tag) {
    const manifest = await readFile(new URL('../../dovetail/Cargo.toml', import.meta.url), 'utf8');
    tag = `v${manifest.match(/^version = "([^"]+)"$/m)[1]}`;
  }
  if (!/^v[0-9]+\.[0-9]+\.[0-9]+(?:[-+][a-zA-Z0-9.-]+)?$/.test(tag)) {
    throw new Error(`Invalid release tag: ${tag}`);
  }
  const files = new Map();
  for (const [name, marker, replacement] of [
    ['install.sh', 'version=latest', `version=${tag}`],
    ['install.ps1', "$Version = 'latest'", `$Version = '${tag}'`],
  ]) {
    const source = await readFile(new URL(`../installers/${name}`, import.meta.url), 'utf8');
    if (source.split(marker).length !== 2) throw new Error(`Missing or duplicate version marker in ${name}`);
    files.set(`/${name}`, source.replace(marker, replacement));
  }
  return files;
}
