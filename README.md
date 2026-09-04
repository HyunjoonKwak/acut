# Photo Desk

> **가져와 고르고, 제자리에 놓는다.**

Photo Desk는 macOS에서 대규모 사진 라이브러리를 탐색하고, 촬영일과 폴더를 교정한 뒤
`작업대 → 내사진 → 공용` 흐름으로 정리하는 오프라인 우선 데스크톱 앱입니다.

**기술:** Tauri 2 · Rust · React 19 · TypeScript · Tailwind CSS 4 · SQLite

## 사진 시스템에서의 역할

| 앱 | 정본 역할 |
|---|---|
| **Photo Backup** | 폰 사진을 NAS 1차 구역으로 수집 |
| **Photo Desk** | 물리 파일 정리, 촬영일 교정, 내사진·공용 발행 |
| **우리집 사진관** | 읽기 중심 감상과 논리적 큐레이션 |

사진은 `폰 → NAS 1차 구역 → Photo Desk 작업대 → 내사진·공용 → 우리집 사진관` 순서로
흐릅니다. Photo Desk의 내사진·공용 폴더와 NAS의 대응 폴더는 **Synology Drive Client**가
동기화합니다. Drive는 이 저장소의 코드가 아니며, 물리 파일 관리의 정본은 Photo Desk입니다.

경로 계약과 운영 경계는 [ECOSYSTEM.md](ECOSYSTEM.md), 변경 원칙은
[역할 경계](docs/ROLE_BOUNDARIES.md)를 먼저 확인하세요.

## 주요 기능

- 9개 탐색 갈래: 모든 사진, 앨범, 스마트 앨범, 검색, 태그, 사람, 달력, 위치, 카메라
- 대용량 가상 격자, 타임라인, 상세 정보, Quick Look, 나란히 보기
- 완전 중복·줄인 사본·연사·저품질 후보 검토와 남김/제외 판정
- 촬영일 dry-run 감사, 자동·수동·일괄 교정, 배치 journal과 undo
- 사진 이동·복사, 내사진→공용 발행, SHA-256 원장 기반 재실행 중복 방지
- 폴더 생성·이름변경·이동·복사·휴지통, 충돌 미리보기와 배치 undo
- 날짜 폴더 이름 감사와 시간 간격 기반 이벤트 자동 발견
- 얼굴·유사 장면·텍스트 검색을 위한 로컬 AI, GPS 지도와 오프라인 국가 판정
- NAS 1차 구역 rsync 받기·검증·안전 정리, 별도 백업 대상으로 사본 생성

## 안전 원칙

- 스캔, 검색, 판정, dry-run은 원본 파일을 바꾸지 않습니다.
- 내사진·공용의 이동·이름변경·휴지통은 Drive를 통해 NAS에도 반영될 수 있어 경고합니다.
- 파일·폴더 작업은 실행 전 충돌을 보여 주고 batch journal에 기록합니다.
- 파일 복사 undo는 크기와 SHA-256이 실행 직후 사본과 같은 경우에만 삭제합니다.
- 이동·이름변경·휴지통 undo는 옮긴 직후 기록한 크기·수정 시각과 같은 파일만 되돌립니다.
- 같은 볼륨의 폴더 이름변경·이동·휴지통은 파일 내용을 읽지 않고 이름·크기·수정 시각으로
  기록·대조합니다. 폴더 복사와 볼륨 간 이동만 SHA-256 manifest로 사본을 검증합니다.
- 촬영일 교정은 JPEG/JPG에 EXIF 세 필드와 mtime을 기록합니다. 그 밖의 포맷은 파일 내부
  메타데이터를 바꾸지 않고 mtime과 Photo Desk override만 기록합니다.
- JPEG 교정 전 원본 바이트를 백업하며 undo 시 SHA-256을 확인해 복원합니다.
- 라이브러리 루트, 부모→자식 순환, 오프라인 볼륨, 루트 밖 심볼릭 링크를 차단합니다.

세부 사용법은 [사용 가이드](docs/USER_GUIDE.md), 전환 검증 근거는
[G1 검증](docs/G1_VALIDATION.md)과 [G2 파일럿](docs/G2_PILOT.md)에 있습니다.

## 설치

[Releases](https://github.com/HyunjoonKwak/photo_desk/releases)에서 최신 DMG를 받아
`Photo Desk.app`을 응용 프로그램에 넣습니다.

현재 개인 사용 빌드는 ad-hoc 서명이며 Apple 공증을 받지 않습니다. 신뢰한 이 저장소의
릴리스가 macOS 격리로 차단될 때만 다음을 한 번 실행합니다.

```bash
xattr -dr com.apple.quarantine "/Applications/Photo Desk.app"
```

공개 배포에서는 Developer ID 서명과 공증을 사용해야 합니다. 기존 `스마트 폴더`
데이터는 첫 실행 시 현재 bundle identifier의 데이터 위치로 자동 이전됩니다.

## 기본 사용 흐름

1. 왼쪽 앨범 패널의 **라이브러리 추가**에서 폴더와 영역(작업대·내사진·공용·기타)을 정합니다.
2. 작업대 사진을 스캔하고 중복·연사·품질 후보를 검토합니다.
3. 필요하면 **촬영일 감사**로 근거와 기록 범위를 먼저 확인한 뒤 교정합니다.
4. 사진을 선택해 이벤트 이름으로 내사진에 이동하거나 **이동·복사**로 임의 목적지에 보냅니다.
5. 내사진→공용은 복사가 기본이며 개인 원본을 유지합니다.
6. 가장 최근의 지원 작업은 상태바의 **되돌리기** 또는 `⌘Z`로 취소합니다.

폴더 메뉴에서는 촬영일 감사·교정, 일반 폴더 작업, 폴더 이름 감사, 작업대 이벤트 자동 발견을
실행할 수 있습니다.

## 단축키

앱에서 `?`를 누르면 현재 단축키 표를 볼 수 있습니다. 주요 키는 다음과 같습니다.

| 키 | 동작 |
|---|---|
| 방향키 | 사진 이동, `Shift`와 함께 범위 선택 |
| `Space` | 크게 보기 |
| `P` / `X` / `F` | 남김 / 제외 / 즐겨찾기 |
| `0`–`5` | 별점 |
| `C` | 선택 사진 나란히 보기 |
| `I` | 정보 패널 |
| `⌘A` | 현재 불러온 사진 전체 선택 |
| `⌘Z` | 가장 최근 지원 작업 되돌리기 |
| `Esc` | 대화상자 닫기 또는 선택 해제 |

## 설정

설정은 일반, 라이브러리, 탐색, 뷰어, AI, 데이터베이스, 백업, NAS, 고급, 정보의
10개 구역입니다. NAS는 DSM Web API 업로드가 아니라 rsync pull/verify/purge 흐름입니다.
Cron이나 MCP 제어 서버는 현재 앱에 없습니다.

## 개발과 검증

```bash
npm install
npm run tauri:dev
npm test
npm run lint
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
npm run tauri:build
```

- 백엔드 Tauri 명령: 125개 (`src-tauri/src/api/`)
- 프론트엔드: `src/v2/`
- 영역 정의: `src/v2/areaItems.ts`
- 앱 아이콘 원본: `src-tauri/icons/icon.svg`

## 데이터 위치

`~/Library/Application Support/com.acut.media/`

- `acut-v2.db` — 라이브러리, 설정, 작업·발행 원장
- `thumbs/`, `previews/` — 파생 이미지 캐시
- `models/` — 로컬 AI 모델
- `backups/` — 데이터베이스 백업

촬영일을 교정한 JPEG의 undo 원본은 해당 라이브러리 안
`.acut/capture-date-backups/<batch>/`에 보관됩니다. 성공적으로 undo하면 지워지며,
undo 전에는 복구 근거이므로 자동 삭제하지 않습니다.

bundle identifier `com.acut.media`는 데이터 연결 키이므로 변경하지 않습니다.

## License

MIT
