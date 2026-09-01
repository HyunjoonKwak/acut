#!/usr/bin/env node

// 버전을 다섯 파일에서 한꺼번에 올린다:
// package.json · package-lock.json(뿌리 + packages."") · src-tauri/tauri.conf.json ·
// src-tauri/Cargo.toml · src-tauri/Cargo.lock(acut 패키지)
// 사용: node scripts/bump-version.mjs [major|minor|patch|x.y.z]

import { readFileSync, writeFileSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, "..");
const type = process.argv[2] || "patch";

function bumpVersion(version, type) {
  if (/^\d+\.\d+\.\d+$/.test(type)) return type;
  const [major, minor, patch] = version.split(".").map(Number);
  switch (type) {
    case "major": return `${major + 1}.0.0`;
    case "minor": return `${major}.${minor + 1}.0`;
    case "patch": return `${major}.${minor}.${patch + 1}`;
    default: throw new Error(`Unknown bump type: ${type}`);
  }
}

const pkgPath = resolve(root, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf-8"));
const oldVersion = pkg.version;
const newVersion = bumpVersion(oldVersion, type);

pkg.version = newVersion;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");

const lockPath = resolve(root, "package-lock.json");
const lock = JSON.parse(readFileSync(lockPath, "utf-8"));
lock.version = newVersion;
if (lock.packages && lock.packages[""]) lock.packages[""].version = newVersion;
writeFileSync(lockPath, JSON.stringify(lock, null, 2) + "\n");

const tauriConfPath = resolve(root, "src-tauri/tauri.conf.json");
const tauriConf = JSON.parse(readFileSync(tauriConfPath, "utf-8"));
tauriConf.version = newVersion;
writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + "\n");

const cargoPath = resolve(root, "src-tauri/Cargo.toml");
let cargo = readFileSync(cargoPath, "utf-8");
cargo = cargo.replace(/^version\s*=\s*"[^"]*"/m, `version = "${newVersion}"`);
writeFileSync(cargoPath, cargo);

// Cargo.lock — [[package]] name = "acut" 바로 아래 version 만 바꾼다
const cargoLockPath = resolve(root, "src-tauri/Cargo.lock");
const cargoLockEntry = /(name = "acut"\nversion = ")[^"]*(")/;
let cargoLock = readFileSync(cargoLockPath, "utf-8");
// **바뀌었는지가 아니라 있는지를 본다.** 결과가 같다고 «못 찾았다»로 치면, 이미 그
// 판인 버전을 그대로 지정할 때(`release.mjs 0.8.0`) 없는 잘못을 만든다 (2026-09-02).
if (!cargoLockEntry.test(cargoLock)) {
  throw new Error("Cargo.lock 에서 acut 항목을 못 찾았습니다");
}
cargoLock = cargoLock.replace(cargoLockEntry, `$1${newVersion}$2`);
writeFileSync(cargoLockPath, cargoLock);

console.log(`${oldVersion} → ${newVersion}`);
