#!/usr/bin/env node

/**
 * 오프라인 지명 데이터를 만든다 — GeoNames cities15000 + admin1 이름표.
 *
 * 앱은 이 결과 파일만 읽는다. 빌드나 실행 중에는 절대 내려받지 않는다.
 * 데이터를 갱신할 때만 사람이 이 스크립트를 손으로 돌린다:
 *
 *     node scripts/build-geodata.mjs
 *
 * 같은 원본이면 같은 결과가 나오게(결정적) 정렬·서식을 고정한다. 결과의
 * sha256 을 MANIFEST 에 적어 두고 러스트 시험이 그것을 검증한다.
 *
 * 라이선스: 원본은 GeoNames, CC BY 4.0. 앱과 배포물에 고지해야 한다.
 */

import { createHash } from "crypto";
import { mkdirSync, writeFileSync, readFileSync, existsSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { execFileSync } from "child_process";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = resolve(root, "src-tauri/data/geonames");
const tmpDir = resolve(root, ".geodata-tmp");

const SOURCES = {
  cities: "https://download.geonames.org/export/dump/cities15000.zip",
  admin1: "https://download.geonames.org/export/dump/admin1CodesASCII.txt",
};

/** GeoNames 열 번호(1부터) — readme.txt 기준 */
const COL = { name: 2, lat: 5, lon: 6, cc: 9, admin1: 11, admin2: 12, pop: 15 };

function fetchTo(url, dest) {
  console.log(`받는 중: ${url}`);
  execFileSync("curl", ["-sSL", "-o", dest, url], { stdio: ["ignore", "inherit", "inherit"] });
}

function main() {
  mkdirSync(outDir, { recursive: true });
  mkdirSync(tmpDir, { recursive: true });

  const zip = resolve(tmpDir, "cities15000.zip");
  const admin1Path = resolve(tmpDir, "admin1CodesASCII.txt");
  if (!existsSync(zip)) fetchTo(SOURCES.cities, zip);
  if (!existsSync(admin1Path)) fetchTo(SOURCES.admin1, admin1Path);
  execFileSync("unzip", ["-o", "-q", zip, "-d", tmpDir]);

  // admin1 코드 → 영문 이름 (KR.41 → Gyeonggi-do)
  const admin1Name = new Map();
  for (const line of readFileSync(admin1Path, "utf-8").split("\n")) {
    if (!line.trim()) continue;
    const [code, name] = line.split("\t");
    if (code && name) admin1Name.set(code, name.trim());
  }

  const rows = [];
  for (const line of readFileSync(resolve(tmpDir, "cities15000.txt"), "utf-8").split("\n")) {
    if (!line.trim()) continue;
    const f = line.split("\t");
    const lat = Number(f[COL.lat - 1]);
    const lon = Number(f[COL.lon - 1]);
    const cc = (f[COL.cc - 1] || "").trim();
    if (!Number.isFinite(lat) || !Number.isFinite(lon) || !cc) continue;
    const a1code = (f[COL.admin1 - 1] || "").trim();
    rows.push([
      lat.toFixed(4),
      lon.toFixed(4),
      (f[COL.name - 1] || "").trim(),
      cc,
      a1code,
      admin1Name.get(`${cc}.${a1code}`) ?? "",
      (f[COL.admin2 - 1] || "").trim(),
      String(Number(f[COL.pop - 1] || 0) | 0),
    ]);
  }

  // 결정적 정렬 — 원본 순서가 바뀌어도 결과가 같게
  rows.sort((a, b) =>
    a[3].localeCompare(b[3], "en") ||
    a[4].localeCompare(b[4], "en") ||
    a[2].localeCompare(b[2], "en") ||
    a[0].localeCompare(b[0], "en") ||
    a[1].localeCompare(b[1], "en"),
  );

  const header = "# lat\tlon\tname\tcc\tadmin1_code\tadmin1_name\tadmin2_code\tpopulation\n";
  const body = rows.map((r) => r.join("\t")).join("\n") + "\n";
  const tsv = header + body;
  const tsvPath = resolve(outDir, "cities.tsv");
  writeFileSync(tsvPath, tsv);

  const sha = createHash("sha256").update(tsv).digest("hex");
  // 원본 파일의 날짜를 판 번호로 — 사람이 언제 받은 데이터인지 알 수 있게
  const stamp = new Date().toISOString().slice(0, 10);
  const manifest = {
    format_version: 1,
    dataset: "geonames-cities15000",
    dataset_version: stamp,
    source: SOURCES.cities,
    admin1_source: SOURCES.admin1,
    license: "CC BY 4.0 (GeoNames)",
    record_count: rows.length,
    sha256: sha,
    fields: ["lat", "lon", "name", "cc", "admin1_code", "admin1_name", "admin2_code", "population"],
  };
  writeFileSync(resolve(outDir, "MANIFEST.json"), JSON.stringify(manifest, null, 2) + "\n");

  console.log(`cities.tsv — ${rows.length.toLocaleString()}행 · ${(tsv.length / 1048576).toFixed(2)} MB`);
  console.log(`sha256 ${sha}`);
  console.log(`판 ${stamp}`);
  console.log(`임시 파일은 ${tmpDir} 에 남습니다 — 지워도 됩니다.`);
}

main();
