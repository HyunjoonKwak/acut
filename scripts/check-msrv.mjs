#!/usr/bin/env node

// 선언한 최소 러스트 버전이 실제 의존성이 요구하는 값 이상인지 본다.
//
// 낮게 적어 두면 «이 버전에서 빌드된다»는 거짓말이 된다. 그 툴체인을 쓰는
// 사람은 알 수 없는 오류를 만나고, 우리는 왜 그런지 모른다. (2026-09-01)

import { execFileSync } from "child_process";
import { readFileSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = resolve(root, "src-tauri/Cargo.toml");

/** "1.88" 처럼 자리가 모자란 표기도 받는다 — 크레이트마다 적는 방식이 다르다 */
const parse = (v) => {
  const n = String(v).trim().split(".").map((x) => Number(x) || 0);
  return [n[0] ?? 0, n[1] ?? 0, n[2] ?? 0];
};
const cmp = (a, b) => {
  const [x, y] = [parse(a), parse(b)];
  return x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
};

const declared = readFileSync(manifest, "utf-8").match(/^rust-version\s*=\s*"([^"]+)"/m)?.[1];
if (!declared) {
  console.error("✗ src-tauri/Cargo.toml 에 rust-version 이 없습니다.");
  process.exit(1);
}

const meta = JSON.parse(
  execFileSync("cargo", ["metadata", "--manifest-path", manifest, "--format-version", "1"], {
    cwd: root,
    maxBuffer: 64 * 1024 * 1024,
    encoding: "utf-8",
  }),
);

let worst = { name: null, version: "0.0.0" };
for (const p of meta.packages) {
  if (p.name === "acut" || !p.rust_version) continue;
  if (cmp(p.rust_version, worst.version) > 0) worst = { name: p.name, version: p.rust_version };
}

if (cmp(declared, worst.version) < 0) {
  console.error(
    `✗ 선언한 최소 러스트 ${declared} 보다 ${worst.name} 이(가) 더 높은 ${worst.version} 을 요구합니다.\n` +
      `  Cargo.toml 의 rust-version 을 ${worst.version} 이상으로 올리거나, 그 의존성을 낮은 판으로 고정하세요.`,
  );
  process.exit(1);
}
console.log(`최소 러스트 ${declared} — 가장 높이 요구하는 것은 ${worst.name} ${worst.version}`);
