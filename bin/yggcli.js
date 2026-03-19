#!/usr/bin/env node

const fs = require("fs");
const os = require("os");
const path = require("path");
const https = require("https");
const { spawnSync } = require("child_process");

const PKG = require("../package.json");
const REPO = process.env.YGGCLI_REPO || "https://github.com/yggdrasilhq/yggcli";
const VERSION = process.env.YGGCLI_VERSION || `v${PKG.version}`;
const CACHE_ROOT =
  process.env.YGGCLI_CACHE_DIR || path.join(os.homedir(), ".cache", "yggcli");

function detectPlatform() {
  const nodePlatform = process.platform;
  const nodeArch = process.arch;

  let platform;
  if (nodePlatform === "linux") {
    platform = "linux";
  } else if (nodePlatform === "android") {
    platform = "android";
  } else {
    fail(`unsupported platform: ${nodePlatform}`);
  }

  let arch;
  if (nodeArch === "x64") {
    arch = "amd64";
  } else if (nodeArch === "arm64") {
    arch = "arm64";
  } else {
    fail(`unsupported architecture: ${nodeArch}`);
  }

  return { platform, arch };
}

function releaseUrls(platform, arch) {
  const base = `${REPO.replace(/\/$/, "")}/releases/download/${VERSION}`;
  const urls = [`${base}/yggcli-${platform}-${arch}`];
  if (platform === "linux" && arch === "amd64") {
    urls.push(`${base}/yggcli`);
  }
  return urls;
}

function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
}

function fail(message) {
  console.error(`[yggcli-npm] ${message}`);
  process.exit(1);
}

function download(url, dest) {
  return new Promise((resolve, reject) => {
    const request = https.get(
      url,
      {
        headers: {
          "User-Agent": "yggcli-npm-launcher"
        }
      },
      (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          download(res.headers.location, dest).then(resolve, reject);
          return;
        }
        if (res.statusCode !== 200) {
          reject(new Error(`download failed: ${res.statusCode} ${res.statusMessage}`));
          return;
        }
        const file = fs.createWriteStream(dest, { mode: 0o755 });
        res.pipe(file);
        file.on("finish", () => file.close(resolve));
        file.on("error", reject);
      }
    );
    request.on("error", reject);
  });
}

function buildFromSource() {
  const installer = path.join(__dirname, "..", "install.sh");
  const result = spawnSync("bash", [installer], {
    stdio: "inherit",
    env: {
      ...process.env,
      VERSION,
      YGGCLI_REPO: REPO
    }
  });
  if (result.status !== 0) {
    fail(`install fallback failed with status ${result.status}`);
  }
  return path.join(os.homedir(), ".local", "bin", "yggcli");
}

async function downloadFirst(urls, dest) {
  let lastError;
  for (const url of urls) {
    try {
      console.error(`[yggcli-npm] downloading ${url}`);
      await download(url, dest);
      return;
    } catch (err) {
      lastError = err;
    }
  }
  throw lastError || new Error("no release URL candidates succeeded");
}

async function main() {
  const { platform, arch } = detectPlatform();
  const cacheDir = path.join(CACHE_ROOT, VERSION);
  const binaryPath = path.join(cacheDir, `yggcli-${platform}-${arch}`);
  ensureDir(cacheDir);

  if (!fs.existsSync(binaryPath)) {
    try {
      await downloadFirst(releaseUrls(platform, arch), binaryPath);
      fs.chmodSync(binaryPath, 0o755);
    } catch (err) {
      console.error(`[yggcli-npm] release download unavailable: ${err.message}`);
      const fallbackPath = buildFromSource();
      runBinary(fallbackPath);
      return;
    }
  }

  runBinary(binaryPath);
}

function runBinary(binaryPath) {
  const result = spawnSync(binaryPath, process.argv.slice(2), {
    stdio: "inherit"
  });
  if (result.error) {
    fail(result.error.message);
  }
  process.exit(result.status === null ? 1 : result.status);
}

main().catch((err) => fail(err.message));
