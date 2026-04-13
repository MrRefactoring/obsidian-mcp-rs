import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { execSync } from "node:child_process";
import { existsSync } from "node:fs";

vi.mock("node:child_process");
vi.mock("node:fs");

// ── helpers ───────────────────────────────────────────────────────────────────

function mockProcess(platform: string, arch: string) {
  Object.defineProperty(process, "platform", { value: platform, configurable: true });
  Object.defineProperty(process, "arch", { value: arch, configurable: true });
}

const origPlatform = process.platform;
const origArch = process.arch;

// ── detectPlatform ────────────────────────────────────────────────────────────

describe("detectPlatform", () => {
  beforeEach(() => {
    vi.mocked(execSync).mockReturnValue("ldd (GNU libc) 2.31");
  });

  afterEach(() => {
    mockProcess(origPlatform, origArch);
  });

  it("darwin arm64", async () => {
    mockProcess("darwin", "arm64");
    const { detectPlatform } = await import("./platform");
    expect(detectPlatform()).toBe("darwin-arm64");
  });

  it("darwin x64", async () => {
    mockProcess("darwin", "x64");
    const { detectPlatform } = await import("./platform");
    expect(detectPlatform()).toBe("darwin-x64");
  });

  it("win32 arm64", async () => {
    mockProcess("win32", "arm64");
    const { detectPlatform } = await import("./platform");
    expect(detectPlatform()).toBe("win32-arm64");
  });

  it("win32 x64", async () => {
    mockProcess("win32", "x64");
    const { detectPlatform } = await import("./platform");
    expect(detectPlatform()).toBe("win32-x64");
  });

  it("linux arm64 (always linux-arm64, musl irrelevant)", async () => {
    mockProcess("linux", "arm64");
    const { detectPlatform } = await import("./platform");
    expect(detectPlatform()).toBe("linux-arm64");
  });

  it("linux x64 glibc", async () => {
    mockProcess("linux", "x64");
    vi.mocked(execSync).mockReturnValue("ldd (GNU libc) 2.31");
    const { detectPlatform } = await import("./platform");
    expect(detectPlatform()).toBe("linux-x64");
  });

  it("linux x64 musl", async () => {
    mockProcess("linux", "x64");
    vi.mocked(execSync).mockReturnValue("musl libc (x86_64)");
    const { detectPlatform } = await import("./platform");
    expect(detectPlatform()).toBe("linux-x64-musl");
  });

  it("throws on unsupported platform", async () => {
    mockProcess("freebsd", "x64");
    const { detectPlatform } = await import("./platform");
    expect(() => detectPlatform()).toThrow("Unsupported platform: freebsd/x64");
  });
});

// ── detectMusl ────────────────────────────────────────────────────────────────

describe("detectMusl", () => {
  afterEach(() => {
    vi.mocked(execSync).mockReset();
  });

  it("returns true when ldd output contains 'musl'", async () => {
    vi.mocked(execSync).mockReturnValue("musl libc (x86_64)");
    const { detectMusl } = await import("./platform");
    expect(detectMusl()).toBe(true);
  });

  it("returns false when ldd output does not contain 'musl'", async () => {
    vi.mocked(execSync).mockReturnValue("ldd (GNU libc) 2.31");
    const { detectMusl } = await import("./platform");
    expect(detectMusl()).toBe(false);
  });

  it("returns false when execSync throws", async () => {
    vi.mocked(execSync).mockImplementation(() => { throw new Error("not found"); });
    const { detectMusl } = await import("./platform");
    expect(detectMusl()).toBe(false);
  });
});

// ── resolveBinaryPath ─────────────────────────────────────────────────────────

describe("resolveBinaryPath", () => {
  beforeEach(() => {
    vi.mocked(existsSync).mockReturnValue(false);
  });

  it("returns path from require.resolve candidate when file exists", async () => {
    vi.mocked(existsSync).mockImplementation(
      (p) => String(p).includes("darwin-arm64") && !String(p).endsWith("package.json"),
    );
    const { resolveBinaryPath } = await import("./platform");
    const result = resolveBinaryPath("darwin-arm64");
    expect(result).toContain("darwin-arm64");
    expect(result).toContain("obsidian-mcp-rs");
  });

  it("appends .exe for win32 platforms", async () => {
    vi.mocked(existsSync).mockImplementation((p) => String(p).endsWith(".exe"));
    const { resolveBinaryPath } = await import("./platform");
    const result = resolveBinaryPath("win32-x64");
    expect(result).toContain(".exe");
  });

  it("falls back to __dirname-relative path when require.resolve fails but file exists", async () => {
    vi.mocked(existsSync).mockImplementation(
      (p) => String(p).includes("linux-x64") && !String(p).endsWith("package.json"),
    );
    const { resolveBinaryPath } = await import("./platform");
    // With the fallback candidate the path is built from __dirname
    const result = resolveBinaryPath("linux-x64");
    expect(result).toContain("linux-x64");
  });

  it("throws when no binary is found", async () => {
    vi.mocked(existsSync).mockReturnValue(false);
    const { resolveBinaryPath } = await import("./platform");
    expect(() => resolveBinaryPath("linux-x64")).toThrow(
      "Could not find the obsidian-mcp-rs binary",
    );
  });

  it("linux-x64 binary name has no .exe", async () => {
    vi.mocked(existsSync).mockImplementation((p) => String(p).includes("linux-x64"));
    const { resolveBinaryPath } = await import("./platform");
    const result = resolveBinaryPath("linux-x64");
    expect(result).not.toContain(".exe");
    expect(result).toContain("obsidian-mcp-rs");
  });
});
