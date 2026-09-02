# Gallery → Desk G2 실제 사진 사본 파일럿

- 실행일: 2026-09-02 (Asia/Seoul)
- 운영 원본: 등록된 PHOTO 라이브러리의 Samsung JPEG 한 장(읽기 전용)
- 원본 크기: 2,301,374 bytes
- 원본 SHA-256: `2ed6dcb365971b0eca5e20eba07f4b910c6161f533eb034abbc2b411353fca09`
- 같은 볼륨 device: `16777233`
- cross-volume device: `16777247` (`T7 2T SSD`)
- 운영 Photo Desk DB: 읽거나 수정하지 않음
- Photo Gallery 코드·설정·배포: 변경하지 않음

파일럿은 `g2_pilot::real_photo_copy_round_trip` ignored test로 재현한다. 원본 JPEG를
격리된 임시 내사진/공용 라이브러리와 실제 외장 볼륨 파일럿 폴더에 복사하고, 별도
SQLite DB에서만 P0 작업을 실행한다.

## 1. 최종 성공 증거

### 촬영일 교정과 undo

| 항목 | 결과 |
|---|---|
| batch | 1 |
| 교정 전 SHA-256 | `2ed6dcb365971b0eca5e20eba07f4b910c6161f533eb034abbc2b411353fca09` |
| 교정 후 SHA-256 | `04b8ecacc8cb84b05e5853f2bf99ed85f776bf4d5c1e199eca1a80d2f5ba76cf` |
| 재판독 촬영일 | `1582604449` |
| undo 결과 | 1 성공, 0 실패 |
| undo 후 SHA-256 | `2ed6dcb365971b0eca5e20eba07f4b910c6161f533eb034abbc2b411353fca09` |

DateTimeOriginal·DateTimeDigitized·TIFF DateTime을 같은 값으로 기록했고, 카메라
제조사·모델·GPS와 디코딩 화소가 교정 전후 동일함을 함께 검증했다. 마지막 undo는
원본 JPEG 바이트 SHA를 정확히 복원했다.

### 내사진 → 공용 발행

| 항목 | 결과 |
|---|---|
| 첫 실행 batch | 2 |
| 첫 실행 | 1 completed, 0 failed |
| 두 번째 실행 | 0 completed, 1 already-published |
| 개인 원본 | 유지, SHA 일치 |
| 공용 사본 | SHA 일치 |
| undo | 공용 사본 1건 제거, 개인 원본 유지 |

### 폴더 이동·복사·undo

공통 manifest SHA-256:
`ac17487375345dab003c97290d6e0099a76aa3b0d13335c0edc0f6277910432d`

| 작업 | batch | preview | 실행 | undo |
|---|---:|---|---|---|
| 같은 볼륨 이동 | 4 | cross-volume=false | 1 성공 | 1 성공 |
| 같은 볼륨 복사 | 5 | cross-volume=false | 1 성공 | 1 성공 |
| 실제 cross-volume 복사 | 6 | cross-volume=true | 1 성공 | 1 성공 |
| 실제 cross-volume 이동 | 7 | cross-volume=true | 1 성공 | 1 성공 |

각 작업에서 실제 JPEG와 XMP sidecar가 함께 이동·복사됐고, destination SHA와 undo 후
source SHA가 원본과 일치했다. 최종 파일럿 디렉터리도 제거됐다.

## 2. 파일럿이 발견한 실패와 대책

### 실패 1 — 기존 EXIF가 있는 JPEG의 촬영일 재판독 불일치

- 증상: 바이트는 변경됐지만 DateTimeOriginal 재판독값이 기존 값으로 남음.
- 원인: ImageIO metadata merge에서 기존 바이너리 EXIF가 추가 metadata보다 우선함.
- 대책: 표준 TIFF/EXIF 세 tag가 있는 JPEG는 APP1 EXIF의 고정 길이 ASCII 값만
  원자적으로 제자리 수정. EXIF가 전혀 없으면 세 tag를 가진 최소 APP1을 직접 추가한다.
  두 경우 모두 압축 화소를 유지하며 기존 EXIF의 MakerNote·GPS·segment 배치는 보존한다.
- 회귀 검사: EXIF 없는 JPEG 최초·재교정과 기존 EXIF JPEG 재교정을 모두 검사.

### 실패 2 — APFS → exFAT manifest 불일치

- 증상: cross-volume 사본 manifest가 원본과 다름.
- 원인: macOS가 exFAT에 만든 `._` AppleDouble 파일과 한글 NFC/NFD 표현 차이.
- 대책: manifest와 복사에서 AppleDouble만 제외하고 경로를 NFC로 정규화. XMP 등
  사용자 sidecar는 계속 hash와 복사 대상에 포함.

### 실패 3 — exFAT cross-volume undo의 DirectoryNotEmpty

- 증상: 사본 SHA 검증은 성공했으나 undo 삭제가 code 66으로 중단됨.
- 원인: 삭제 도중 exFAT에 AppleDouble 항목이 뒤늦게 생성되고, read_dir 이름과
  삭제 syscall의 유니코드 정규형이 달라짐.
- 대책: 검증된 작업 경로만 bottom-up으로 반복 제거하고, 삭제 경로를 NFC로
  정규화하며 AppleDouble sibling도 정리.

최종 실행 결과는 `1 passed, 0 failed`이고 evidence의 `failures` 배열은 비어 있다.

최종 회귀 검사는 Rust `540 passed / 0 failed / 21 ignored`, strict clippy, 프론트
logic `116 passed`, UI `60 passed`, production Tauri app·DMG build까지 통과했다.
최종 DMG SHA-256은
`16916a2584cc30dbe851911839f558d97cc22dd9dbe5adf0db8be1b132352efc`다.
개인용 ad-hoc 서명이며 Apple 공증은 수행하지 않았다.

## 3. 재현 명령

```bash
PHOTO_DESK_G2_SOURCE_JPEG='/path/to/read-only-real-photo.jpg' \
PHOTO_DESK_G2_CROSS_VOLUME_ROOT='/Volumes/another-physical-volume' \
cargo test g2_pilot::real_photo_copy_round_trip -- --ignored --nocapture
```

두 환경변수의 device id가 같으면 테스트는 시작 단계에서 실패한다. 실패한 실행은
조사를 위해 외장 볼륨의 `.photo-desk-g2-*` 파일럿 폴더를 남기고, 성공한 실행은
자동으로 정리한다.
