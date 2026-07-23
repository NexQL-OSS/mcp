#!/usr/bin/env node
/**
 * npx shim — resolves the platform-specific optionalDependency binary
 * (same pattern as esbuild: `@nexql/mcp-<platform>-<arch>`).
 */
'use strict';

const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const PACKAGE_VERSION = require('../package.json').version;

/** @type {Record<string, string>} */
const knownPackages = {
  'darwin arm64': '@nexql/mcp-darwin-arm64',
  'darwin x64': '@nexql/mcp-darwin-x64',
  'linux arm64': '@nexql/mcp-linux-arm64',
  'linux x64': '@nexql/mcp-linux-x64',
  'win32 x64': '@nexql/mcp-win32-x64',
};

/**
 * @returns {{ pkg: string, subpath: string }}
 */
function pkgAndSubpathForCurrentPlatform() {
  const key = `${process.platform} ${process.arch}`;
  const pkg = knownPackages[key];
  if (!pkg) {
    throw new Error(
      `Unsupported platform: ${key} (${os.type()} ${os.release()}). ` +
        `Supported: ${Object.keys(knownPackages).join(', ')}`,
    );
  }
  const subpath =
    process.platform === 'win32' ? 'bin/nexql-mcp.exe' : 'bin/nexql-mcp';
  return { pkg, subpath };
}

/**
 * @param {string} pkg
 * @param {string} subpath
 * @returns {string}
 */
function resolveBinary(pkg, subpath) {
  try {
    return require.resolve(`${pkg}/${subpath}`);
  } catch {
    // Package may be present but binary not yet published into the stub.
    try {
      const pkgRoot = path.dirname(require.resolve(`${pkg}/package.json`));
      const candidate = path.join(pkgRoot, subpath);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    } catch {
      // fall through
    }
    throw new Error(
      `Platform binary not installed (${pkg}@${PACKAGE_VERSION}).\n` +
        '  npm install nexql-mcp\n' +
        '  # or build from source:\n' +
        '  cargo install --path crates/nexql-mcp\n' +
        'Platform packages are published by release CI; stubs under npm/packages/ are placeholders until then.',
    );
  }
}

function main() {
  let pkg;
  let subpath;
  try {
    ({ pkg, subpath } = pkgAndSubpathForCurrentPlatform());
  } catch (e) {
    console.error(`nexql-mcp: ${e.message}`);
    process.exit(1);
  }

  let binaryPath;
  try {
    binaryPath = resolveBinary(pkg, subpath);
  } catch (e) {
    console.error(`nexql-mcp: ${e.message}`);
    process.exit(1);
  }

  if (!fs.existsSync(binaryPath)) {
    console.error(`nexql-mcp: binary missing at ${binaryPath}`);
    process.exit(1);
  }

  const result = spawnSync(binaryPath, process.argv.slice(2), {
    stdio: 'inherit',
    windowsHide: true,
  });

  if (result.error) {
    console.error(`nexql-mcp: failed to spawn ${binaryPath}: ${result.error.message}`);
    process.exit(1);
  }

  process.exit(result.status === null ? 1 : result.status);
}

main();
