#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { detectPlatform, resolveBinaryPath } from "./platform";

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
