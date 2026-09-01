#!/usr/bin/env node

// 릴리스 문지기 — 검증이 전부 통과해야 버전을 올리고 빌드한다.
// 실패하면 버전 파일은 손대지 않은 채 그대로 멈춘다.
// 사용: node scripts/release.mjs [major|minor|patch|x.y.z]  (기본 patch)

import { spawnSync } from "child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "fs";
import { resolve, dirname, join } from "path";
import { fileURLToPath } from "url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const type = process.argv[2] || "patch";

/** 버전이 적힌 파일들 — bump-version.mjs 가 손대는 것과 같아야 한다 */
const VERSION_FILES = [
  "package.json",
  "package-lock.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
];

/**
 * 실패해도 던진다 — `process.exit` 로 끝내면 되돌리는 자리를 지나치지 못한다.
 *
 * 버전을 올린 뒤 빌드나 고지 검사가 넘어지면, 손댄 파일이 그대로 남아 다음
 * 실행이 «버전 일치» 문지기에서 걸리거나 엉뚱한 판으로 올라간다 (2026-09-01).
 */
function run(title, cmd, args) {
  console.log(`\n=== ${title}: ${cmd} ${args.join(" ")}`);
  const r = spawnSync(cmd, args, { cwd: root, stdio: "inherit" });
  if (r.status !== 0) throw new Error(`${title} 실패 (종료 코드 ${r.status ?? "없음"})`);
}

/**
 * 알려진 취약점이 있는 크레이트를 쓰고 있지 않은지 본다.
 *
 * `cargo audit` 이 없으면 릴리스를 막지는 않는다 — 없다고 릴리스가 불가능해지면
 * 사람들은 이 걸음을 지우게 된다. 대신 무엇을 못 했는지 또렷이 남긴다.
 */
function auditCrates() {
  console.log("\n=== 크레이트 취약점 점검: cargo audit");
  const probe = spawnSync("cargo", ["audit", "--version"], { cwd: root, stdio: "ignore" });
  if (probe.status !== 0) {
    console.warn(
      "  ⚠ cargo audit 이 없어 건너뜁니다 — `cargo install cargo-audit` 로 설치하면 다음부터 확인합니다.",
    );
    return;
  }
  run("취약점 점검", "cargo", ["audit", "--file", "src-tauri/Cargo.lock"]);
}

/** 앱에 실리는 npm 의존성에는 high 이상 취약점이 없어야 한다. */
function auditFrontend() {
  run("프론트 의존성 취약점 점검", "npm", ["audit", "--omit=dev", "--audit-level=high"]);
}

function main() {
  run("버전 일치", "node", ["scripts/check-versions.mjs"]);
  run("최소 러스트 버전", "node", ["scripts/check-msrv.mjs"]);
  auditCrates();
  auditFrontend();
  run("프론트 테스트", "npm", ["test"]);
  run("린트", "npm", ["run", "lint"]);
  run("프론트 빌드", "npm", ["run", "build"]);
  run("러스트 테스트", "cargo", [
    "test", "--manifest-path", "src-tauri/Cargo.toml", "--all-targets", "--locked",
  ]);
  run("클리피", "cargo", [
    "clippy", "--manifest-path", "src-tauri/Cargo.toml", "--all-targets", "--locked", "--", "-D", "warnings",
  ]);

  // 여기까지 왔으면 안전 — 이제서야 버전을 올리고 빌드한다
  run("버전 올리기", "node", ["scripts/bump-version.mjs", type]);
  run("앱 빌드", "npx", ["tauri", "build"]);
  checkNotice();
  checkMacDistribution();
}

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
  // 던져야 한다 — process.exit 로 끝내면 버전을 되돌리는 자리를 지나친다
  if (!existsSync(macos)) throw new Error(`번들 폴더가 없습니다: ${macos}`);
  const apps = readdirSync(macos).filter((n) => n.endsWith(".app"));
  if (!apps.length) throw new Error(`${macos} 안에 .app 이 없습니다.`);
  const want = readFileSync(resolve(root, "NOTICE"), "utf-8");
  for (const app of apps) {
    const path = join(macos, app, "Contents/Resources/NOTICE");
    if (!existsSync(path)) {
      throw new Error(
        `${app} 안에 NOTICE 가 없습니다 — tauri.conf.json 의 bundle.resources 를 확인하세요.\n  있어야 할 자리: ${path}`,
      );
    }
    if (readFileSync(path, "utf-8") !== want) {
      throw new Error(`${app} 안의 NOTICE 가 루트의 것과 다릅니다: ${path}`);
    }
    console.log(`  ${app}/Contents/Resources/NOTICE — 루트와 같음`);
  }
}

/**
 * 공개 배포물이 Gatekeeper가 신뢰하는 서명과 공증 티켓을 모두 가졌는지 확인한다.
 *
 * ad-hoc 서명도 `codesign --verify`만은 통과하므로 그것만 봐서는 안 된다. 실제
 * Gatekeeper 평가와 stapler 검증까지 통과해야 릴리스가 끝난다. 인증서·공증 환경이
 * 없는 개발자는 `npm run tauri:build`로 로컬 번들을 만들 수 있지만 `npm run release`는
 * 공개 릴리스로 가장하지 않고 여기서 멈춘다.
 */
function checkMacDistribution() {
  console.log("\n=== macOS 서명·공증 확인");
  const macos = resolve(root, "src-tauri/target/release/bundle/macos");
  const apps = existsSync(macos) ? readdirSync(macos).filter((n) => n.endsWith(".app")) : [];
  if (!apps.length) throw new Error(`${macos} 안에 .app 이 없습니다.`);
  for (const app of apps) {
    const path = join(macos, app);
    run("코드 서명 확인", "codesign", ["--verify", "--deep", "--strict", path]);
    run("Gatekeeper 확인", "spctl", ["--assess", "--type", "execute", "--verbose=2", path]);
    run("공증 티켓 확인", "xcrun", ["stapler", "validate", path]);
  }
}

// 버전 파일을 통째로 기억해 둔다 — 무엇이 실패하든 원래대로 돌린다
const saved = new Map();
for (const rel of VERSION_FILES) {
  const path = resolve(root, rel);
  if (existsSync(path)) saved.set(path, readFileSync(path));
}

let failed = null;
try {
  main();
} catch (e) {
  failed = e;
} finally {
  if (failed) {
    let restored = 0;
    for (const [path, body] of saved) {
      if (!readFileSync(path).equals(body)) {
        writeFileSync(path, body);
        restored++;
      }
    }
    console.error(`\n✗ ${failed.message}`);
    console.error(
      restored > 0
        ? `  버전 파일 ${restored}개를 원래대로 돌렸습니다.`
        : "  버전 파일은 손대지 않았습니다.",
    );
    process.exit(1);
  }
}
console.log("\n릴리스 번들 준비 끝 — 태그·업로드는 따로 하세요.");
