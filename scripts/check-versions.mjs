#!/usr/bin/env node

// 다섯 파일의 버전이 같은지 확인한다 — 다르면 어떤 파일이 어긋났는지 말하고 1 로 끝난다.
// package-lock 이 0.3.3 으로 남아 있던 사고(v0.5.0 점검)를 다시 겪지 않기 위한 문지기.

import { readFileSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(resolve(root, p), "utf-8");

const versions = {
  "package.json": JSON.parse(read("package.json")).version,
  "package-lock.json": JSON.parse(read("package-lock.json")).version,
  'package-lock.json packages.""': JSON.parse(read("package-lock.json")).packages?.[""]?.version,
  "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
  "src-tauri/Cargo.toml": read("src-tauri/Cargo.toml").match(/^version\s*=\s*"([^"]*)"/m)?.[1],
  "src-tauri/Cargo.lock (acut)": read("src-tauri/Cargo.lock").match(/name = "acut"\nversion = "([^"]*)"/)?.[1],
};

const values = [...new Set(Object.values(versions))];
if (values.length === 1 && values[0]) {
  console.log(`버전 일치: ${values[0]}`);
  process.exit(0);
}
console.error("버전이 어긋났습니다:");
for (const [file, v] of Object.entries(versions)) console.error(`  ${file}: ${v ?? "(못 읽음)"}`);
process.exit(1);
