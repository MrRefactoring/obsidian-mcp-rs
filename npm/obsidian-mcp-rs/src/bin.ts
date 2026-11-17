#!/usr/bin/env node

import { spawn } from "node:child_process";
import { detectPlatform, resolveBinaryPath } from "./platform";

/**
 * Forward client shutdown and restart signals. SIGKILL and SIGSTOP cannot be caught.
 */
const FORWARDED: NodeJS.Signals[] = ["SIGINT", "SIGTERM", "SIGHUP"];

function main(): void {
  const platform = detectPlatform();
  const binaryPath = resolveBinaryPath(platform);
  const args = process.argv.slice(2);

  // `spawn` lets Node run signal handlers while the server runs. `spawnSync`
  // blocks them. If the wrapper dies and stdin stays open, the child can remain.
  const child = spawn(binaryPath, args, {
    stdio: "inherit",
    env: process.env,
  });

  for (const signal of FORWARDED) {
    process.on(signal, () => {
      // Forward the signal and wait for the child to exit.
      child.kill(signal);
    });
  }

  child.on("error", (error) => {
    console.error(`obsidian-mcp-rs: could not start ${binaryPath}`);
    console.error(error.message);
    process.exit(1);
  });

  child.on("exit", (code, signal) => {
    // Report the child's exit status to the wrapper's supervisor.
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 0);
  });
}

main();
