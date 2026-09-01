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
  // 한국 도시의 한글 이름만 받는다. 전 세계 대체 이름은 수백 MB 라 넣지 않는다 —
  // 해외 지명은 영문이 오히려 알아보기 쉽다 (Honolulu).
  altKr: "https://download.geonames.org/export/dump/alternatenames/KR.zip",
  admin2: "https://download.geonames.org/export/dump/admin2Codes.txt",
};

/** GeoNames 열 번호(1부터) — readme.txt 기준 */
const COL = { id: 1, name: 2, lat: 5, lon: 6, cc: 9, admin1: 11, admin2: 12, pop: 15 };

/**
 * 한국 도시의 한글 이름 — alternateNames 의 ko 줄에서 고른다.
 *
 * 한 도시에 ko 이름이 여럿이면 «선호» 표시된 것, 그다음 짧은 것을 쓴다.
 * 역사 이름(isHistoric)과 약자(isShortName)는 버린다.
 */
function koreanNames(path) {
  const best = new Map();
  for (const line of readFileSync(path, "utf-8").split("\n")) {
    if (!line) continue;
    const f = line.split("\t");
    // alternateNameId, geonameid, isolanguage, alternate name, isPreferred, isShort, isColloquial, isHistoric
    if (f[2] !== "ko" || !f[1] || !f[3]) continue;
    if (f[7] === "1" || f[5] === "1") continue;
    const id = f[1].trim();
    const name = f[3].trim();
    const preferred = f[4] === "1";
    const prev = best.get(id);
    if (!prev || (preferred && !prev.preferred) || (preferred === prev.preferred && name.length < prev.name.length)) {
      best.set(id, { name, preferred });
    }
  }
  return new Map([...best].map(([id, v]) => [id, v.name]));
}

function fetchTo(url, dest) {
  console.log(`받는 중: ${url}`);
  execFileSync("curl", ["-sSL", "-o", dest, url], { stdio: ["ignore", "inherit", "inherit"] });
}

function main() {
  mkdirSync(outDir, { recursive: true });
  mkdirSync(tmpDir, { recursive: true });

  const zip = resolve(tmpDir, "cities15000.zip");
  const admin1Path = resolve(tmpDir, "admin1CodesASCII.txt");
  const altZip = resolve(tmpDir, "KR-alternatenames.zip");
  const admin2Path = resolve(tmpDir, "admin2Codes.txt");
  if (!existsSync(zip)) fetchTo(SOURCES.cities, zip);
  if (!existsSync(admin1Path)) fetchTo(SOURCES.admin1, admin1Path);
  if (!existsSync(altZip)) fetchTo(SOURCES.altKr, altZip);
  if (!existsSync(admin2Path)) fetchTo(SOURCES.admin2, admin2Path);
  execFileSync("unzip", ["-o", "-q", zip, "-d", tmpDir]);
  execFileSync("unzip", ["-o", "-q", altZip, "-d", tmpDir]);
  const ko = koreanNames(resolve(tmpDir, "KR.txt"));

  // 시군구 이름 — 도시 이름은 동까지 내려가서(제주시의 «Ara-dong») 시군구로 쓸 수 없다.
  // 한국만 채운다: 해외는 도시 이름이 오히려 알아보기 쉽다 (Honolulu).
  const admin2Name = new Map();
  for (const line of readFileSync(admin2Path, "utf-8").split("\n")) {
    const f = line.split("\t");
    if (!f[0]?.startsWith("KR.") || !f[1]) continue;
    // 한글 이름이 있으면 그것을, 없으면 영문을 쓴다
    admin2Name.set(f[0].trim(), (ko.get((f[3] || "").trim()) || f[1].trim()).replace(/[\t\r\n]/g, " "));
  }

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
    const name = (f[COL.name - 1] || "").trim();
    const koName = cc === "KR" ? (ko.get((f[COL.id - 1] || "").trim()) ?? "") : "";
    const a2code = (f[COL.admin2 - 1] || "").trim();
    const a2name = cc === "KR" ? (admin2Name.get(`KR.${a1code}.${a2code}`) ?? "") : "";
    rows.push([
      lat.toFixed(4),
      lon.toFixed(4),
      name,
      cc,
      a1code,
      admin1Name.get(`${cc}.${a1code}`) ?? "",
      a2name,
      String(Number(f[COL.pop - 1] || 0) | 0),
      // 탭·줄바꿈이 들어오면 칸이 어긋난다 — 원본을 믿지 않는다
      koName.replace(/[\t\r\n]/g, " "),
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

  const header = "# lat\tlon\tname\tcc\tadmin1_code\tadmin1_name\tadmin2_name\tpopulation\tname_ko\n";
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
    korean_name_source: SOURCES.altKr,
    admin2_source: SOURCES.admin2,
    license: "CC BY 4.0 (GeoNames)",
    record_count: rows.length,
    sha256: sha,
    fields: ["lat", "lon", "name", "cc", "admin1_code", "admin1_name", "admin2_name", "population", "name_ko"],
    admin2_names: rows.filter((r) => r[6]).length,
    korean_names: rows.filter((r) => r[8]).length,
  };
  writeFileSync(resolve(outDir, "MANIFEST.json"), JSON.stringify(manifest, null, 2) + "\n");

  console.log(`cities.tsv — ${rows.length.toLocaleString()}행 · ${(tsv.length / 1048576).toFixed(2)} MB`);
  console.log(`sha256 ${sha}`);
  console.log(`판 ${stamp}`);
  console.log(`임시 파일은 ${tmpDir} 에 남습니다 — 지워도 됩니다.`);
}

main();
