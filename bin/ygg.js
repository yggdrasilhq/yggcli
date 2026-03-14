#!/usr/bin/env node
const fs = require('fs');
const os = require('os');
const path = require('path');
const {spawnSync} = require('child_process');

const pkg = require('../package.json');
const tag = `v${pkg.version}`;
const platform = process.platform;
const arch = process.arch;

function assetName() {
  if (platform === 'linux' && arch === 'x64') return 'yggcli-linux-amd64';
  throw new Error(`Unsupported platform for now: ${platform}/${arch}`);
}

function cachePath() {
  const base = process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache');
  return path.join(base, 'yggcli', tag, assetName());
}

function releaseUrl() {
  return `https://github.com/yggdrasilhq/yggcli/releases/download/${tag}/${assetName()}`;
}

function download(url, dest) {
  fs.mkdirSync(path.dirname(dest), {recursive: true});
  const result = spawnSync('curl', ['-L', '--fail', '-A', '@ygg/cli', '-o', dest, url], {stdio: 'inherit'});
  if (result.status !== 0) {
    fs.rmSync(dest, {force: true});
    throw new Error(`Download failed: ${url}`);
  }
}

async function ensureBinary() {
  const dest = cachePath();
  if (!fs.existsSync(dest)) {
    console.error(`Downloading yggcli ${tag}...`);
    download(releaseUrl(), dest);
  }
  fs.chmodSync(dest, 0o755);
  return dest;
}

(async () => {
  try {
    const bin = await ensureBinary();
    const result = spawnSync(bin, process.argv.slice(2), {stdio: 'inherit'});
    process.exit(result.status === null ? 1 : result.status);
  } catch (err) {
    console.error(String(err.message || err));
    process.exit(1);
  }
})();
