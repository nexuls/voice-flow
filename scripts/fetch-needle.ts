#!/usr/bin/env bun
/**
 * Downloads the official prebuilt Needle engine (libneedle.a + needle.h) for
 * the build target from Cactus-Compute/needle3 on Hugging Face. Files are
 * pinned to a repository revision and verified against SHA-256 checksums.
 *
 *   bun run fetch:needle              # host platform
 *   bun run fetch:needle linux-arm64  # explicit platform folder
 *
 * Output: src-tauri/vendor/needle/<platform>/ (git-ignored).
 */
import { createHash } from "node:crypto";
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = "Cactus-Compute/needle3";
const REVISION = "d6c7d6a2e9655505e0f8e23339356dd1705900eb";
const HEADER_SHA256 = "a27dac4b3239eead91d124b7f7dbce5495c13019903fd817b68c16d39b055a99";

/** SHA-256 of libneedle.a per platform folder (Hugging Face LFS object ids). */
const LIB_SHA256: Record<string, string> = {
  "linux-x86_64": "2f281877e24fed2658187a32d9a26e74c32d4f273ac9a2b36180b3a6e85942da",
  "linux-arm64": "d54f0d83963c56eb79181929827c9f9ff405891d50cd12974c2b0f6485dd07af",
  "macos-arm64": "7d1368a4a42a745aba055f310913c3ed6d929dee98a89dcb955b9032b2335700",
  "windows-x86_64": "22d95a38f34c225a6af115c40069ea61360766479222862f1fea8b615ea7718a",
  "windows-arm64": "aa4f3fc2ee6614bb34c42a06fd2f08b23c5c1b941c11612bff0947f66112edf4",
};

function hostPlatform(): string {
  const os = { linux: "linux", darwin: "macos", win32: "windows" }[process.platform as string];
  const arch = { x64: "x86_64", arm64: "arm64" }[process.arch as string];
  if (!os || !arch) throw new Error(`unsupported host ${process.platform}/${process.arch}`);
  return `${os}-${arch}`;
}

const sha256 = (data: Uint8Array) => createHash("sha256").update(data).digest("hex");

async function fetchVerified(path: string, expected: string): Promise<Uint8Array> {
  const url = `https://huggingface.co/${REPO}/resolve/${REVISION}/${path}`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`GET ${url}: HTTP ${res.status}`);
  const data = new Uint8Array(await res.arrayBuffer());
  const actual = sha256(data);
  if (actual !== expected) {
    throw new Error(`checksum mismatch for ${path}\n  expected ${expected}\n  actual   ${actual}`);
  }
  return data;
}

async function upToDate(dir: string, libHash: string): Promise<boolean> {
  try {
    const [lib, header] = await Promise.all([
      readFile(join(dir, "libneedle.a")),
      readFile(join(dir, "needle.h")),
    ]);
    return sha256(lib) === libHash && sha256(header) === HEADER_SHA256;
  } catch {
    return false;
  }
}

async function main() {
  const platform = process.argv[2] ?? hostPlatform();
  const libHash = LIB_SHA256[platform];
  if (!libHash) {
    throw new Error(
      `no prebuilt Needle engine for ${platform}; supported: ${Object.keys(LIB_SHA256).join(", ")}`,
    );
  }
  const dir = join(
    dirname(fileURLToPath(import.meta.url)),
    "..",
    "src-tauri",
    "vendor",
    "needle",
    platform,
  );
  if (await upToDate(dir, libHash)) {
    console.log(`Needle engine for ${platform} is up to date (${dir})`);
    return;
  }
  console.log(`Fetching Needle engine for ${platform} @ ${REVISION.slice(0, 12)}…`);
  const [lib, header] = await Promise.all([
    fetchVerified(`${platform}/libneedle.a`, libHash),
    fetchVerified(`${platform}/needle.h`, HEADER_SHA256),
  ]);
  // Write to a temporary directory and rename it into place, so an
  // interrupted run never leaves a half-written engine behind.
  const tmp = `${dir}.tmp-${process.pid}`;
  await rm(tmp, { recursive: true, force: true });
  await mkdir(tmp, { recursive: true });
  await writeFile(join(tmp, "libneedle.a"), lib);
  await writeFile(join(tmp, "needle.h"), header);
  await writeFile(join(tmp, "REVISION"), `${REPO}@${REVISION}\n`);
  await rm(dir, { recursive: true, force: true });
  await rename(tmp, dir);
  console.log(`Verified and installed to ${dir}`);
}

main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
