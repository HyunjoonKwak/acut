#!/usr/bin/env node

// 릴리스가 어디서 넘어져도 버전 파일이 원래대로 돌아오는지 본다.
//
// 버전을 올린 **뒤** 단계(앱 빌드·고지 검사)를 일부러 넘어뜨린다. 그때 파일이
// 손댄 채 남으면 다음 실행이 «버전 일치» 문지기에 걸리거나 엉뚱한 판으로 올라간다.
// 실제 빌드는 몇 분이 걸리므로, 릴리스 대본을 그대로 두고 그것이 부르는 명령만
// 가짜로 바꿔 끼운다.

import { execFileSync } from "child_process";
import { mkdtempSync, readFileSync, writeFileSync, cpSync, rmSync, mkdirSync, chmodSync } from "fs";
import { tmpdir } from "os";
import { resolve, dirname, join } from "path";
import { fileURLToPath } from "url";
import { strict as assert } from "assert";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const FILES = [
  "package.json",
  "package-lock.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
];

/**
 * 릴리스 대본을 임시 사본에서 돌린다 — 진짜 저장소는 건드리지 않는다.
 *
 * `breakAt` 이 넘어뜨릴 명령, `bundleNotice` 가 가짜 빌드 결과에 고지를 둘지.
 */
function runInSandbox(breakAt, bundleNotice = false) {
  const box = mkdtempSync(join(tmpdir(), "acut-release-"));
  try {
    mkdirSync(join(box, "scripts"), { recursive: true });
    mkdirSync(join(box, "src-tauri"), { recursive: true });
    for (const f of [...FILES, "NOTICE"]) {
      cpSync(resolve(root, f), join(box, f));
    }
    for (const f of ["release.mjs", "check-versions.mjs", "bump-version.mjs", "check-msrv.mjs"]) {
      cpSync(resolve(root, "scripts", f), join(box, "scripts", f));
    }

    // 오래 걸리는 단계는 곧바로 성공하는 시늉으로 바꾼다.
    // `breakAt` 로 지정한 단계만 넘어뜨린다.
    const fakeBin = join(box, "bin");
    mkdirSync(fakeBin);
    for (const cmd of ["npm", "cargo", "npx", "codesign", "spctl", "xcrun"]) {
      const fails = breakAt === cmd;
      // `cargo metadata` 와 `cargo audit` 은 릴리스가 실제로 읽는 값을 내야 한다 —
      // 이 시험이 보는 것은 «버전 파일이 돌아오나»이지 그 검사들이 아니다
      const body =
        cmd === "cargo"
          ? `#!/bin/sh\ncase "$1" in\n  metadata) echo '{"packages":[]}'; exit 0 ;;\n  audit) exit 0 ;;\nesac\necho "[가짜 cargo] $@"\nexit ${fails ? 1 : 0}\n`
          : `#!/bin/sh\necho "[가짜 ${cmd}] $@"\nexit ${fails ? 1 : 0}\n`;
      writeFileSync(join(fakeBin, cmd), body);
      chmodSync(join(fakeBin, cmd), 0o755);
    }

    // 가짜 빌드는 아무것도 만들지 않는다 — 고지 검사가 볼 결과물을 미리 놓아 준다
    if (bundleNotice) {
      const app = join(box, "src-tauri/target/release/bundle/macos/Photo Desk.app/Contents/Resources");
      mkdirSync(app, { recursive: true });
      cpSync(resolve(root, "NOTICE"), join(app, "NOTICE"));
    }

    const before = Object.fromEntries(FILES.map((f) => [f, readFileSync(join(box, f), "utf-8")]));
    let status = 0;
    try {
      execFileSync(process.execPath, ["scripts/release.mjs", "minor"], {
        cwd: box,
        env: { ...process.env, PATH: `${fakeBin}:${process.env.PATH}` },
        stdio: "pipe",
      });
    } catch (e) {
      status = e.status ?? 1;
    }
    const after = Object.fromEntries(FILES.map((f) => [f, readFileSync(join(box, f), "utf-8")]));
    return { status, before, after };
  } finally {
    rmSync(box, { recursive: true, force: true });
  }
}

// 1) 버전을 올린 뒤 앱 빌드(npx tauri build)가 넘어진다
{
  const { status, before, after } = runInSandbox("npx");
  assert.equal(status, 1, "빌드가 실패했는데 릴리스가 성공으로 끝났습니다");
  for (const f of FILES) {
    assert.equal(after[f], before[f], `${f} 가 손댄 채 남았습니다`);
  }
  console.log("✓ 앱 빌드 실패 — 버전 파일 원상 복구");
}

// 2) 버전을 올리기 전(러스트 테스트)에 넘어져도 마찬가지
{
  const { status, before, after } = runInSandbox("cargo");
  assert.equal(status, 1);
  for (const f of FILES) {
    assert.equal(after[f], before[f], `${f} 가 손댄 채 남았습니다`);
  }
  console.log("✓ 검증 실패 — 버전 파일 그대로");
}

// 3) 고지가 빠진 채 빌드가 «성공»해도 릴리스는 실패하고 버전이 돌아온다
{
  const { status, before, after } = runInSandbox(null, false);
  assert.equal(status, 1, "고지가 없는데 릴리스가 성공으로 끝났습니다");
  for (const f of FILES) {
    assert.equal(after[f], before[f], `${f} 가 손댄 채 남았습니다`);
  }
  console.log("✓ 고지 검사 실패 — 버전 파일 원상 복구");
}

// 4) 모두 통과하면 버전이 실제로 올라간다 — 시험 자체가 헛돌지 않게
{
  const { status, before, after } = runInSandbox(null, true);
  assert.equal(status, 0, "모두 통과했는데 릴리스가 실패했습니다");
  assert.notEqual(after["package.json"], before["package.json"], "버전이 오르지 않았습니다");
  const v = JSON.parse(after["package.json"]).version;
  assert.equal(JSON.parse(after["src-tauri/tauri.conf.json"]).version, v);
  console.log(`✓ 정상 흐름 — 버전이 ${JSON.parse(before["package.json"]).version} → ${v} 로 올라감`);
}

// 5) 빌드는 됐어도 Gatekeeper가 거절하면 공개 릴리스가 아니며 버전도 돌아간다
{
  const { status, before, after } = runInSandbox("spctl", true);
  assert.equal(status, 1, "Gatekeeper가 거절했는데 릴리스가 성공했습니다");
  for (const f of FILES) {
    assert.equal(after[f], before[f], `${f} 가 손댄 채 남았습니다`);
  }
  console.log("✓ Gatekeeper 검사 실패 — 버전 파일 원상 복구");
}

// 6) 서명 자체나 공증 티켓 검사에서 실패해도 같은 문지기가 막는다
for (const command of ["codesign", "xcrun"]) {
  const { status, before, after } = runInSandbox(command, true);
  assert.equal(status, 1, `${command} 검사가 실패했는데 릴리스가 성공했습니다`);
  for (const f of FILES) {
    assert.equal(after[f], before[f], `${f} 가 손댄 채 남았습니다`);
  }
  console.log(`✓ ${command} 검사 실패 — 버전 파일 원상 복구`);
}

console.log("\n릴리스 복구 시험 통과");
