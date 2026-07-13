#!/usr/bin/env node

import { spawn } from "node:child_process";
import { detectPlatform, resolveBinaryPath } from "./platform";

/**
 * What a client sends to shut a server down or restart it. SIGKILL and SIGSTOP
 * are deliberately absent — they cannot be caught, by anyone.
 */
const FORWARDED: NodeJS.Signals[] = ["SIGINT", "SIGTERM", "SIGHUP"];

function main(): void {
  const platform = detectPlatform();
  const binaryPath = resolveBinaryPath(platform);
  const args = process.argv.slice(2);

  // `spawn`, not `spawnSync`. spawnSync blocks the event loop for the entire life
  // of the server, so Node can never run a signal handler: kill this wrapper and
  // the Rust child is orphaned, still holding the user's vault. It is usually
  // masked — with stdio inherited, the child shares the client's stdin pipe and
  // exits on EOF when the client goes away — but a client that dies while that
  // pipe stays open leaves the process behind.
  const child = spawn(binaryPath, args, {
    stdio: "inherit",
    env: process.env,
  });

  for (const signal of FORWARDED) {
    process.on(signal, () => {
      // Pass it on, then wait: the child's own "exit" below settles our status, so
      // the server gets to shut down rather than being cut off mid-write.
      child.kill(signal);
    });
  }

  child.on("error", (error) => {
    console.error(`obsidian-mcp-rs: could not start ${binaryPath}`);
    console.error(error.message);
    process.exit(1);
  });

  child.on("exit", (code, signal) => {
    // Report the child's fate as our own, so whatever is supervising this wrapper
    // sees what actually happened to the server.
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 0);
  });
}

main();
