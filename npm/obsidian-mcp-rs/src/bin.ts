#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

type Platform =
  | "darwin-arm64"
  | "darwin-x64"
  | "linux-arm64"
  | "linux-x64"
  | "linux-x64-musl"
  | "win32-arm64"
  | "win32-x64";

function detectPlatform(): Platform {
  const { platform, arch } = process;

  if (platform === "darwin") {
    return arch === "arm64" ? "darwin-arm64" : "darwin-x64";
  }

  if (platform === "win32") {
    return arch === "arm64" ? "win32-arm64" : "win32-x64";
  }

  if (platform === "linux") {
    const isMusl = detectMusl();
    if (arch === "arm64") return "linux-arm64";
    return isMusl ? "linux-x64-musl" : "linux-x64";
  }

  throw new Error(`Unsupported platform: ${platform}/${arch}`);
}

function detectMusl(): boolean {
  try {
    const { execSync } = require("node:child_process");
    const output = execSync("ldd --version 2>&1 || true", {
      encoding: "utf8",
      stdio: ["pipe", "pipe", "pipe"],
    });
    return output.toLowerCase().includes("musl");
  } catch {
    return false;
  }
}

function resolveBinaryPath(platform: Platform): string {
  const packageName = `@obsidian-mcp-rs/${platform}`;
  const binaryName =
    platform.startsWith("win32") ? "obsidian-mcp-rs.exe" : "obsidian-mcp-rs";

  const candidates = [
    () => {
      const packageDir = require.resolve(
        `${packageName}/package.json`,
      );
      return join(packageDir, "..", binaryName);
    },
    () =>
      join(
        __dirname,
        "..",
        "..",
        "..",
        `${packageName.replace("@obsidian-mcp-rs/", "")}`,
        binaryName,
      ),
  ];

  for (const candidate of candidates) {
    try {
      const p = candidate();
      if (existsSync(p)) return p;
    } catch {
      // continue
    }
  }

  throw new Error(
    `Could not find the obsidian-mcp-rs binary for platform '${platform}'.\n` +
      `Make sure the package '${packageName}' is installed.\n` +
      `Run: npm install ${packageName}`,
  );
}

function main(): void {
  const platform = detectPlatform();
  const binaryPath = resolveBinaryPath(platform);
  const args = process.argv.slice(2);

  const result = spawnSync(binaryPath, args, {
    stdio: "inherit",
    env: process.env,
  });

  if (result.error) {
    throw result.error;
  }

  process.exit(result.status ?? 0);
}

main();
