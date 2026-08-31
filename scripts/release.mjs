#!/usr/bin/env node

// 릴리스 문지기 — 검증이 전부 통과해야 버전을 올리고 빌드한다.
// 실패하면 버전 파일은 손대지 않은 채 그대로 멈춘다.
// 사용: node scripts/release.mjs [major|minor|patch|x.y.z]  (기본 patch)

import { spawnSync } from "child_process";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const type = process.argv[2] || "patch";

function run(title, cmd, args) {
  console.log(`\n=== ${title}: ${cmd} ${args.join(" ")}`);
  const r = spawnSync(cmd, args, { cwd: root, stdio: "inherit" });
  if (r.status !== 0) {
    console.error(`\n✗ ${title} 실패 — 버전은 바꾸지 않았습니다.`);
    process.exit(r.status ?? 1);
  }
}

run("버전 일치", "node", ["scripts/check-versions.mjs"]);
run("프론트 테스트", "npm", ["test"]);
run("린트", "npm", ["run", "lint"]);
run("프론트 빌드", "npm", ["run", "build"]);
run("러스트 테스트", "cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml", "--all-targets"]);
run("클리피", "cargo", ["clippy", "--manifest-path", "src-tauri/Cargo.toml", "--all-targets", "--", "-D", "warnings"]);

// 여기까지 왔으면 안전 — 이제서야 버전을 올리고 빌드한다
run("버전 올리기", "node", ["scripts/bump-version.mjs", type]);
run("앱 빌드", "npx", ["tauri", "build"]);
console.log("\n릴리스 번들 준비 끝 — 태그·업로드는 따로 하세요.");
