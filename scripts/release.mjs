#!/usr/bin/env node

// 릴리스 문지기 — 검증이 전부 통과해야 버전을 올리고 빌드한다.
// 실패하면 버전 파일은 손대지 않은 채 그대로 멈춘다.
// 사용: node scripts/release.mjs [major|minor|patch|x.y.z]  (기본 patch)

import { spawnSync } from "child_process";
import { existsSync, readFileSync, readdirSync } from "fs";
import { resolve, dirname, join } from "path";
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

/**
 * 만들어진 앱 안에 제3자 고지가 실제로 들어 있는지 본다.
 *
 * 설정 화면이 «앱 안의 NOTICE 파일»을 가리키므로, 그 파일이 없으면 화면이
 * 없는 것을 안내하는 셈이 된다. 설정을 빠뜨려도 릴리스가 조용히 성공하지
 * 않도록 빌드 뒤에 직접 확인한다.
 */
function checkNotice() {
  console.log("\n=== 고지 파일 포함 확인");
  const macos = resolve(root, "src-tauri/target/release/bundle/macos");
  if (!existsSync(macos)) {
    console.error(`✗ 번들 폴더가 없습니다: ${macos}`);
    process.exit(1);
  }
  const apps = readdirSync(macos).filter((n) => n.endsWith(".app"));
  if (!apps.length) {
    console.error(`✗ ${macos} 안에 .app 이 없습니다.`);
    process.exit(1);
  }
  const want = readFileSync(resolve(root, "NOTICE"), "utf-8");
  for (const app of apps) {
    const path = join(macos, app, "Contents/Resources/NOTICE");
    if (!existsSync(path)) {
      console.error(
        `✗ ${app} 안에 NOTICE 가 없습니다 — tauri.conf.json 의 bundle.resources 를 확인하세요.\n  있어야 할 자리: ${path}`,
      );
      process.exit(1);
    }
    if (readFileSync(path, "utf-8") !== want) {
      console.error(`✗ ${app} 안의 NOTICE 가 루트의 것과 다릅니다: ${path}`);
      process.exit(1);
    }
    console.log(`  ${app}/Contents/Resources/NOTICE — 루트와 같음`);
  }
}

// 여기까지 왔으면 안전 — 이제서야 버전을 올리고 빌드한다
run("버전 올리기", "node", ["scripts/bump-version.mjs", type]);
run("앱 빌드", "npx", ["tauri", "build"]);
checkNotice();
console.log("\n릴리스 번들 준비 끝 — 태그·업로드는 따로 하세요.");
