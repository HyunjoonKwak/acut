# Gallery → Desk G1 검증 결과

- 검증일: 2026-09-02 (Asia/Seoul)
- 기준 커밋: `811688895660aef6ed2784e69ad82496240dd44b`
- P0 범위: `09be22f4b1c14de04aee027bc5df4613f9f75cce..811688895660aef6ed2784e69ad82496240dd44b`
- 원격 반영: `origin/main`에 P0 8개 커밋 push 완료
- 제외: Photo Gallery 코드·설정·배포, P1 폴더명 감사, 이벤트 자동 발견

## 1. P0 커밋

1. `38b05f11cf0ac934b16a44ee3edcdb04cac906b8` — 촬영일 감사·교정
2. `46c62cedb91f933507b8c63921c2480f9a5d31cd` — 촬영일 cfg 정리
3. `40bd9b0e9f07d16fecfc5a3242ffb302a499cb13` — 이동·복사·공용 발행
4. `ea8ef02824c5f09becce09674d46087085a85003` — 일반 폴더 작업
5. `4ee74aca01aea8752d4a6ee751e2c8ef1cb0afa6` — P0 migration 회귀 검사
6. `9c60dad6952647e7352d5ce3ba1cddce63cf91ab` — 라이브러리 이동 썸네일 무효화
7. `bf56ff177ffea3a26cec3fe1cfd1a185800f72f7` — P0 작업 후 pending 썸네일 생성
8. `811688895660aef6ed2784e69ad82496240dd44b` — 부분 실패 ID 결과 모델

## 2. 자동 검증

| 검사 | 결과 |
|---|---|
| Rust 전체 | 540 passed, 0 failed, 20 ignored |
| strict clippy | 통과 (`-D warnings`) |
| 프론트 logic | 116 passed |
| 프론트 UI | 20 files, 60 tests passed |
| 버전·릴리스 복구 | 통과 |
| ESLint | 통과 |
| production Tauri build | 통과 |
| 깨끗한 HEAD 재현 | 프론트 build 및 `cargo test --no-run` 통과 |

실모델·실DB·실NAS가 필요한 20개 테스트만 명시적으로 ignored다. 기존 미커밋 UX
변경은 P0 커밋에 섞지 않았고 되돌리지 않았다.

## 3. 촬영일 write/undo 범위

| 포맷 | write | undo |
|---|---|---|
| JPEG/JPG | EXIF DateTimeOriginal·DateTimeDigitized·TIFF DateTime + mtime | 원본 바이트 백업, 시각과 DB 복원 |
| HEIC/HEIF·RAW·PNG·TIFF·WebP·동영상 | mtime + Photo Desk override | 시각과 DB override 복원 |

JPEG 이외 포맷은 파일 내부 메타데이터를 쓴다고 표시하지 않는다.

자동 round-trip manifest:

- 경로: `20240102_235958.jpg`
- before SHA-256: `b5c5653ea5c19c0f362492636d72b47487f3ac6f9ac9d1e85b7c6231d03f7c49`
- write SHA-256: `cf09d3fd85f8763e93a0e9b798a86543b2df053176b2a578281a6ca0331f73a7`
- written/rescan timestamp: `1704207598`
- rescan source: EXIF
- undo: before SHA-256으로 바이트 단위 복원

## 4. 이동·복사·폴더 작업

- 촬영일 2건 중 파일 1건 누락: 1 성공/1 실패, 성공 journal만 undo.
- 파일 복사 2건 중 1건 누락: 1 성공/1 실패, 원본 유지, 성공 사본만 undo.
- 폴더 복사 중간 실패: 임시 목적지 제거, 원본과 sidecar 유지.
- cross-volume 이동: 전체 manifest 검증 뒤 전환, DB 실패 시 rollback 사본 복원.
- 중첩 빈 폴더와 1,000개 파일 manifest 일치.
- 내사진→공용 첫 실행 1건 완료, 재실행 0건 완료/1건 already-published.
- 라이브러리 간 이동은 오래된 썸네일 행을 지우고 목적지 pending 썸네일만 만든다.

## 5. DB migration과 rollback

추가 테이블은 `capture_date_journal`, `capture_date_overrides`,
`publication_ledger`, `folder_journal`이며 `CREATE TABLE IF NOT EXISTS`로 멱등 적용한다.

롤백은 최신 P0 batch부터 앱의 undo를 실행한 뒤 이전 바이너리로 내린다. 추가 테이블은
이전 앱이 무시하므로 유지해도 된다. DB까지 과거 상태로 되돌려야 하면 테이블을 직접
삭제하지 말고 사전 DB 백업을 복원한다.

## 6. 빌드 산출물

- 앱: `src-tauri/target/release/bundle/macos/Photo Desk.app`
- DMG: `src-tauri/target/release/bundle/dmg/Photo Desk_0.8.0_aarch64.dmg`
- DMG SHA-256: `c1894e439224d8ba0b89f33b5a28bb538426cd40bc307b15d930afbacf0e48af`
- ad-hoc codesign 검증: 통과
- 공증/Gatekeeper: 개인용 무공증 빌드이므로 배포용 평가는 거부됨

## 7. G2에서 확인할 항목

- 실제 JPEG 사본의 교정 후 undo 원본 SHA 복원
- 내사진→공용 발행과 같은 파일 재실행 중복 방지
- 실제 같은 볼륨 및 서로 다른 물리 볼륨의 폴더 이동·복사·undo
- 실제 Drive/NAS 전파는 운영 전환 없이 별도 후속 검증
