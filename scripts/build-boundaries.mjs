#!/usr/bin/env node

/**
 * 한국 시도(ADM1) 경계를 만든다 — Natural Earth 10m admin_1.
 *
 * 나라는 country-boundaries 크레이트가 판정한다. 그 크레이트에는 시도가 없어서
 * 한국만 따로 넣는다 — 사진 대부분이 한국이고, 전 세계 ADM1 은 14.2MB 라 무겁다.
 *
 * 앱은 결과 파일만 읽는다. 빌드나 실행 중에는 절대 내려받지 않는다:
 *
 *     node scripts/build-boundaries.mjs
 *
 * 원본은 Natural Earth — 퍼블릭 도메인이라 고지 의무가 없지만 출처는 NOTICE 에 적는다.
 */

import { createHash } from "crypto";
import { mkdirSync, writeFileSync, readFileSync, existsSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { execFileSync } from "child_process";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = resolve(root, "src-tauri/data/boundaries");
const tmpDir = resolve(root, ".geodata-tmp");

const SOURCE =
  "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_10m_admin_1_states_provinces.geojson";
/** GeoNames 의 시도 코드표 — 경계와 도시를 잇는 열쇠를 여기서 얻는다 */
const ADMIN1_SOURCE = "https://download.geonames.org/export/dump/admin1CodesASCII.txt";
/** 국가 코드 목록 — 이름은 CLDR(Intl)에서 가져오고 여기서는 코드만 쓴다 */
const COUNTRY_SOURCE = "https://download.geonames.org/export/dump/countryInfo.txt";

/** 좌표 소수 자리 — 3자리는 약 110m. 시도 경계에 그 이상은 필요 없다. */
const PLACES = 3;

/** 링 하나를 줄인다 — 반올림 뒤 이어진 같은 점을 지운다. 점이 모자라면 버린다. */
function thin(ring) {
  const out = [];
  for (const [lon, lat] of ring) {
    const p = [Number(lon.toFixed(PLACES)), Number(lat.toFixed(PLACES))];
    const last = out[out.length - 1];
    if (!last || last[0] !== p[0] || last[1] !== p[1]) out.push(p);
  }
  // 닫힌 링의 마지막 점은 첫 점과 같다 — 러스트 쪽에서 이어 붙이므로 뺀다
  if (out.length > 1) {
    const a = out[0];
    const b = out[out.length - 1];
    if (a[0] === b[0] && a[1] === b[1]) out.pop();
  }
  return out.length >= 3 ? out : null;
}

/** 링의 넓이(도²) — 아주 작은 조각을 걸러내는 데만 쓴다 */
function area(ring) {
  let s = 0;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    s += ring[j][0] * ring[i][1] - ring[i][0] * ring[j][1];
  }
  return Math.abs(s) / 2;
}

/** 6ha 아래 조각은 버린다 — 울릉도(72km²)보다 한참 작은 갯바위만 걸린다 */
const MIN_AREA = 5e-7;

function polygons(geom) {
  const raw = geom.type === "Polygon" ? [geom.coordinates] : geom.coordinates;
  const out = [];
  for (const poly of raw) {
    const rings = poly.map(thin).filter(Boolean);
    if (!rings.length || area(rings[0]) < MIN_AREA) continue;
    out.push(rings);
  }
  return out;
}

function bbox(polys) {
  let minLon = 180, minLat = 90, maxLon = -180, maxLat = -90;
  for (const rings of polys) {
    for (const [lon, lat] of rings[0]) {
      if (lon < minLon) minLon = lon;
      if (lon > maxLon) maxLon = lon;
      if (lat < minLat) minLat = lat;
      if (lat > maxLat) maxLat = lat;
    }
  }
  return [minLon, minLat, maxLon, maxLat];
}

/**
 * 국가 코드 → 이름표.
 *
 * 경계 크레이트는 ISO 코드(KR·JP)만 답한다. 사람에게 보일 이름은 CLDR 에서
 * 가져온다 — 손으로 옮겨 적으면 250개 중 몇은 반드시 틀린다.
 */
function buildCountries() {
  const path = resolve(tmpDir, "countryInfo.txt");
  if (!existsSync(path)) {
    console.log(`받는 중: ${COUNTRY_SOURCE}`);
    execFileSync("curl", ["-sSL", "-o", path, COUNTRY_SOURCE], { stdio: ["ignore", "inherit", "inherit"] });
  }
  const ko = new Intl.DisplayNames(["ko"], { type: "region" });
  if (ko.of("KR") !== "대한민국") {
    throw new Error("이 노드에 한국어 지역 이름이 없습니다(ICU 축소판) — 전체 ICU 가 있는 노드로 실행해 주세요");
  }

  const rows = [];
  for (const line of readFileSync(path, "utf-8").split("\n")) {
    if (!line || line.startsWith("#")) continue;
    const f = line.split("\t");
    const cc = (f[0] || "").trim();
    const en = (f[4] || "").trim();
    if (!/^[A-Z]{2}$/.test(cc) || !en) continue;
    let name = en;
    try {
      // CLDR 에 없는 코드는 of() 가 코드를 그대로 돌려준다 — 그때는 영문을 쓴다
      const k = ko.of(cc);
      if (k && k !== cc) name = k;
    } catch {
      /* 영문 이름을 쓴다 */
    }
    rows.push([cc, name.replace(/[\t\r\n]/g, " "), en.replace(/[\t\r\n]/g, " ")]);
  }
  rows.sort((a, b) => a[0].localeCompare(b[0], "en"));
  if (rows.length < 200) throw new Error(`국가 수가 너무 적습니다: ${rows.length}개`);

  const tsv = "# cc\tname\tname_en\n" + rows.map((r) => r.join("\t")).join("\n") + "\n";
  writeFileSync(resolve(outDir, "countries.tsv"), tsv);
  const sha = createHash("sha256").update(tsv).digest("hex");
  writeFileSync(
    resolve(outDir, "COUNTRIES.json"),
    JSON.stringify(
      {
        format_version: 1,
        dataset: "geonames-countryinfo + cldr-ko",
        source: COUNTRY_SOURCE,
        license: "CC BY 4.0 (GeoNames) · Unicode CLDR",
        record_count: rows.length,
        sha256: sha,
        fields: ["cc", "name", "name_en"],
      },
      null,
      2,
    ) + "\n",
  );
  console.log(`countries.tsv — 국가 ${rows.length}개 · sha256 ${sha}`);
}

function main() {
  mkdirSync(outDir, { recursive: true });
  mkdirSync(tmpDir, { recursive: true });

  const src = resolve(tmpDir, "ne10m_admin1.json");
  if (!existsSync(src)) {
    console.log(`받는 중: ${SOURCE}`);
    execFileSync("curl", ["-sSL", "-o", src, SOURCE], { stdio: ["ignore", "inherit", "inherit"] });
  }

  const admin1Path = resolve(tmpDir, "admin1CodesASCII.txt");
  if (!existsSync(admin1Path)) {
    console.log(`받는 중: ${ADMIN1_SOURCE}`);
    execFileSync("curl", ["-sSL", "-o", admin1Path, ADMIN1_SOURCE], { stdio: ["ignore", "inherit", "inherit"] });
  }
  // GeoNames 의 geonameId → 시도 코드. 이름은 두 데이터가 서로 다르게 적어서
  // («Jeju-do» vs «Jeju-teukbyeoljachido») 열쇠로 못 쓴다 — 숫자 id 로 잇는다.
  const byGeonameId = new Map();
  for (const line of readFileSync(admin1Path, "utf-8").split("\n")) {
    const f = line.split("\t");
    if (f[0]?.startsWith("KR.") && f[3]) byGeonameId.set(f[3].trim(), f[0].split(".")[1]);
  }

  const all = JSON.parse(readFileSync(src, "utf-8"));
  const kr = all.features.filter((f) => f.properties.adm0_a3 === "KOR");
  if (kr.length !== 17) throw new Error(`한국 시도가 17개가 아닙니다: ${kr.length}개`);

  const regions = kr
    .map((f) => {
      const p = f.properties;
      const polys = polygons(f.geometry);
      if (!polys.length) throw new Error(`${p.name} 의 경계가 비었습니다`);
      const gn = byGeonameId.get(String(p.gn_id));
      if (!gn) throw new Error(`${p.name} 을 GeoNames 시도 코드에 잇지 못했습니다 (gn_id=${p.gn_id})`);
      return {
        code: p.iso_3166_2,
        // 도시 표의 admin1_code 와 같은 값 — 두 데이터가 같은 곳을 말하는지 대조한다
        geonames_admin1: gn,
        // 표시용 이름은 한글 — 없으면 영문으로 떨어진다
        name: (p.name_ko || p.name || "").trim(),
        bbox: bbox(polys),
        polys,
      };
    })
    // 결정적 순서 — 원본 순서가 바뀌어도 결과가 같게
    .sort((a, b) => a.code.localeCompare(b.code, "en"));

  const missing = regions.filter((r) => !r.code || !r.name || !r.geonames_admin1);
  if (missing.length) throw new Error(`코드나 이름이 빠진 시도: ${missing.length}개`);

  const doc = {
    format_version: 1,
    dataset: "natural-earth-10m-admin1-kr",
    source: SOURCE,
    admin1_source: ADMIN1_SOURCE,
    license: "Public domain (Natural Earth)",
    coordinate_places: PLACES,
    regions,
  };
  const json = JSON.stringify(doc) + "\n";
  const path = resolve(outDir, "kr_admin1.json");
  writeFileSync(path, json);

  const sha = createHash("sha256").update(json).digest("hex");
  const rings = regions.reduce((n, r) => n + r.polys.reduce((m, p) => m + p.length, 0), 0);
  const points = regions.reduce(
    (n, r) => n + r.polys.reduce((m, p) => m + p.reduce((k, ring) => k + ring.length, 0), 0),
    0,
  );
  writeFileSync(
    resolve(outDir, "MANIFEST.json"),
    JSON.stringify(
      {
        format_version: 1,
        dataset: doc.dataset,
        source: SOURCE,
        license: doc.license,
        region_count: regions.length,
        ring_count: rings,
        point_count: points,
        sha256: sha,
      },
      null,
      2,
    ) + "\n",
  );

  buildCountries();

  console.log(`kr_admin1.json — 시도 ${regions.length}개 · 링 ${rings}개 · 점 ${points.toLocaleString()}개 · ${(json.length / 1048576).toFixed(2)} MB`);
  console.log(`sha256 ${sha}`);
}

main();
