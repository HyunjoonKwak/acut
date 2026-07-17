# 에이컷 (A-Cut)

> **B컷은 버리고, A컷만 NAS로.**

사진·영상을 가져오고, 고르고, 정리해서 Synology NAS로 올리는 macOS 로컬 컬링 스테이션.

**Tech Stack:** Tauri 2 + Rust + React 19 + TypeScript + TailwindCSS 4 + SQLite

---

## 다운로드 및 설치

[Releases](https://github.com/HyunjoonKwak/acut/releases)에서 최신 `.dmg`를 받아 설치합니다.

앱은 ad-hoc 서명되어 있어 다운로드 직후 "손상됨" 경고가 뜰 수 있습니다. 터미널에서 한 번만 실행해 주세요:

```bash
xattr -cr /Applications/에이컷.app
```

> 기존 "스마트 폴더" 사용자: 첫 실행 시 라이브러리(DB·설정)가 자동으로 이전됩니다.

---

## 화면 구성 — 워크플로우가 곧 내비게이션

좌측 아이콘 레일 5개 영역이 사용 순서 그대로 배치되어 있습니다:
**가져오고 → 고르고 → 정리하고 → 올린다.**

### 🗂 작업대
전 과정을 지휘하는 메인 컨트롤 화면.
- **파이프라인 보드**: 가져오기 → 고르기 → 정리 → 올리기 4스테이션의 잔여량과 바로가기
- **장치 알림**: SD카드 연결 시 "작업대에 올리기" 원클릭 가져오기 (등록+스캔)
- **스마트 정리**: 중복 → 연사 → 폴더 정리를 순서대로 안내하는 위저드
- 최근 작업 3건 + 즉시 되돌리기, 자동화(감시/스케줄/MCP) 상태

### 🖼 라이브러리
모든 사진·영상 탐색과 메타데이터 작업.
- 보기 탭: **격자 / 리스트 / 지도**(GPS 클러스터)
- 썸네일 크기 슬라이더 80~320px (`⌘+` / `⌘−`)
- **인스펙터**: EXIF · 태그 칩 · 앨범 · **코멘트** — 코멘트는 검색으로도 찾을 수 있고 그리드에 💬 배지로 표시
- 사이드바: 소스 폴더 · 날짜/폴더 그룹 · **태그/앨범 필터** · **장치**(내장 디스크 포함, 전체 검색/폴더 선택/안전 추출)
- 지원 포맷 40종+: JPEG, PNG, HEIC, AVIF, RAW(CR2/CR3/NEF/ARW/DNG 등), MP4, MOV 등

### ✅ 고르기
그룹에서 남길 사진을 고르는 세 가지 모드 — 공통 레이아웃과 단축키.
- **중복**: 3단계 해시(크기 → xxHash64 퀵해시 → SHA-256) 탐지, 보관본 지정
- **연사**: 시간 근접 그룹 + 품질 점수(라플라시안 선명도·히스토그램 노출), 베스트 자동 선정 + `1~9` 수동 지정
- **한 장씩**: 몰입형 리뷰어에서 유지/B컷 마킹, 품질 오버레이, 자동 분석
- 모든 삭제는 **Dry-Run 미리보기 → 휴지통 경유** (영구 삭제 없음)

### 📁 정리
파일과 폴더를 옮기고 다듬는 모든 작업.
- **자동 분류**: 날짜별(EXIF 촬영일)/유형별 — Before-After 미리보기 후 실행
- **폴더**: 트리 탐색 + 용량 분석 + **구조 복사 / 이동 / 이름변경 / 휴지통** (⌘클릭으로 폴더 선택, 드래그&드롭 지원), YYYYMMDD → 날짜 폴더 일괄 정리
- **동기화**: 단방향 미러링, 체크섬 검증, 충돌 해결(덮어쓰기/이름변경/건너뛰기), 프리셋
- **히스토리**: 모든 이동·정리 작업의 배치 단위 되돌리기 (폴더 이동 포함)

### ☁️ NAS
Synology DSM Web API 직접 연동 — 파이프라인의 출구.
- 원격 폴더 브라우징·생성, 촬영일 날짜 폴더 정리, **B컷 제외(A컷만)** 업로드
- SHA-256 원장으로 재업로드 방지, 업로드 완료 배지, 대용량 스트림 업로드, 취소 지원

---

## 단축키

| 화면 | 키 | 동작 |
|---|---|---|
| 고르기 공통 | `J`/`K` 또는 `↑`/`↓` | 그룹 이동 |
| | `Space` | Quick Look 미리보기 |
| | `S` | 그룹 무시 |
| 연사 | `1`~`9` | 베스트 지정 |
| 한 장씩 | `Space` / `K` | B컷 / 유지 마킹 후 다음 |
| | `←`/`→` (`H`/`L`) | 이전/다음 |
| | `?` | 단축키 도움말 |
| 라이브러리 | `⌘+` / `⌘−` | 썸네일 크기 |
| 정리 › 폴더 | `⌘클릭` | 폴더 선택 (복사/이동/이름변경/휴지통) |

---

## 자동화

- **폴더 감시**: notify(FSEvents) 기반 변경 감지 시 자동 스캔 (디바운스 설정 가능)
- **스케줄**: Cron 표현식으로 스캔/정리/동기화 반복 실행
- **MCP 서버**: Unix 도메인 소켓 JSON-RPC (MCP 2024-11-05) — AI 에이전트가 scan, get_stats, detect_duplicates, organize, sync, get_media_list 도구로 직접 제어

---

## 기술 스택

| 레이어 | 기술 |
|--------|------|
| 데스크톱 프레임워크 | Tauri 2 |
| 백엔드 | Rust 2021 (Tokio + Rayon) |
| 프론트엔드 | React 19 + TypeScript 5.9 + Zustand 5 |
| UI | TailwindCSS 4 + Lucide Icons |
| 데이터베이스 | SQLite (rusqlite, WAL) |
| 해싱 | xxHash64 (quick) · SHA-256 (full) |
| 이미지/EXIF | image crate · kamadak-exif |
| 파일 감시 / 스케줄 | notify 7 · tokio-cron-scheduler |
| i18n | i18next (한국어 · English) |

## 개발

```bash
npm install
npm run tauri:dev      # 개발 모드
npm run tauri:build    # 릴리스 빌드 (.app + .dmg, ad-hoc 서명)
npm run version:minor  # 버전 범프 (package.json/Cargo.toml/tauri.conf.json 동기화)
```

- 백엔드: Rust 커맨드 100개 (`src-tauri/src/commands/`)
- 영역 내비게이션: `src/utils/navigation.ts` (5영역 ↔ 뷰 매핑)
- 앱 아이콘 마스터: `src-tauri/icons/icon.svg` → `tauri icon`으로 전 세트 재생성
- 스캔 시 시스템 경로(Library, System, `.app` 번들, node_modules 등) 자동 제외 — 내장 디스크 전체 스캔 가능

## 데이터 위치

`~/Library/Application Support/com.acut.media/`

- `smart_category.db` — 미디어 라이브러리 (SQLite, WAL)
- `config.yaml` — 설정 (atomic write)
- `smart-folder.sock` — MCP 서버 소켓
- 구 식별자(`com.smartcategory.media`)의 데이터는 첫 실행 시 자동 이전됩니다

## License

MIT
