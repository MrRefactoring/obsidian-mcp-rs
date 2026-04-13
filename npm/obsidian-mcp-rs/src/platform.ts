import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

export type Platform =
  | "darwin-arm64"
  | "darwin-x64"
  | "linux-arm64"
  | "linux-x64"
  | "linux-x64-musl"
  | "win32-arm64"
  | "win32-x64";

export function detectPlatform(): Platform {
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

export function detectMusl(): boolean {
  try {
    const output = execSync("ldd --version 2>&1 || true", {
      encoding: "utf8",
      stdio: ["pipe", "pipe", "pipe"],
    }) as string;
    return output.toLowerCase().includes("musl");
  } catch {
    return false;
  }
}

export function resolveBinaryPath(platform: Platform): string {
  const packageName = `@obsidian-mcp-rs/${platform}`;
  const binaryName =
    platform.startsWith("win32") ? "obsidian-mcp-rs.exe" : "obsidian-mcp-rs";

  const candidates = [
    () => {
      const packageDir = require.resolve(`${packageName}/package.json`);
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
