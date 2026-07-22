#!/usr/bin/env node
/**
 * npx shim — resolves the platform-specific optionalDependency binary.
 * Implementation lands in phase 6 (cargo-dist + per-platform npm packages).
 */
const { spawnSync } = require('node:child_process');
const path = require('node:path');

const platform = process.platform;
const arch = process.arch;

const packageMap = {
  'darwin-arm64': '@nexql/mcp-darwin-arm64',
  'darwin-x64': '@nexql/mcp-darwin-x64',
  'linux-arm64': '@nexql/mcp-linux-arm64',
  'linux-x64': '@nexql/mcp-linux-x64',
  'win32-x64': '@nexql/mcp-win32-x64',
};

const key = `${platform}-${arch}`;
const pkg = packageMap[key];

if (!pkg) {
  console.error(`nexql-mcp: unsupported platform ${key}`);
  process.exit(1);
}

let binaryPath;
try {
  binaryPath = require.resolve(path.join(pkg, 'bin/nexql-mcp'));
} catch {
  console.error(
    `nexql-mcp: platform binary not installed (${pkg}).\n` +
      'Run: npm install nexql-mcp\n' +
      'Or build from source: cargo install --path crates/nexql-mcp',
  );
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), { stdio: 'inherit' });
process.exit(result.status ?? 1);
