-- Photo Desk v2 스키마
-- 대규모 로컬 라이브러리를 위한 오프라인 우선 사진 관리자
--
-- 설계 원칙
--   1. 절대경로를 저장하지 않는다. 볼륨 UUID + 볼륨 내 상대경로.
--      macOS 마운트 경로는 불안정하다 (같은 이름 볼륨이 있으면 "PHOTO 1"로 밀림).
--   2. EXIF는 files 안에 인라인. 조인 없이 정렬·필터한다.
--   3. taken_at은 항상 채워지고(폴백 체인), 어디서 나온 값인지 함께 남긴다.
--   4. 고르기 결과는 그룹이 아니라 파일의 속성이다.
--   5. 물리적 위치가 곧 처리 단계다 (작업대 / 내사진 / 공용).

PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------------------
-- 볼륨 — 여러 디스크에 흩어질 수 있다 (운영 SSD · 아카이브 · 백업)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS volumes (
    uuid            TEXT PRIMARY KEY,          -- NSURLVolumeUUIDStringKey
    name            TEXT NOT NULL,             -- 표시용. 바뀔 수 있다
    last_mount_path TEXT,                      -- 마지막으로 본 마운트 지점
    role            TEXT NOT NULL,             -- library | archive | backup
    total_bytes     INTEGER,
    free_bytes      INTEGER,
    is_online       INTEGER NOT NULL DEFAULT 0,
    last_seen_at    INTEGER
);

-- ---------------------------------------------------------------------------
-- 라이브러리 — 등록한 사진 폴더. 여러 개, 서로 다른 디스크에 있어도 된다
--
-- 이 층이 없으면 "지금 열린 폴더" 하나만 알게 되고, 다른 디스크 사진은 목록에는
-- 나오는데 썸네일과 원본을 찾지 못한다. Lap의 albums와 같은 역할이다. 다만
-- Lap은 절대경로를 저장해 마운트 이름이 바뀌면 깨지므로, 우리는 볼륨 UUID +
-- 볼륨 내 상대경로로 나눠 둔다.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS libraries (
    id          INTEGER PRIMARY KEY,
    volume_uuid TEXT NOT NULL REFERENCES volumes(uuid) ON DELETE CASCADE,
    rel_path    TEXT NOT NULL,                 -- 볼륨 최상단이면 빈 문자열
    name        TEXT NOT NULL,                 -- 표시용
    area        INTEGER NOT NULL DEFAULT 1,    -- 0 작업대 · 1 내사진 · 2 공용 · 3 기타
    added_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    scanned_at  INTEGER,
    UNIQUE (volume_uuid, rel_path)
);
CREATE INDEX IF NOT EXISTS idx_libraries_volume ON libraries(volume_uuid);

-- ---------------------------------------------------------------------------
-- 폴더 — 경로 문자열 대신 정규화. inode로 외부 이동을 따라간다
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS folders (
    id          INTEGER PRIMARY KEY,
    volume_uuid TEXT NOT NULL REFERENCES volumes(uuid) ON DELETE CASCADE,
    -- 어느 라이브러리에 속하는가. 썸네일 캐시와 원본 경로를 여기서 푼다.
    library_id  INTEGER REFERENCES libraries(id) ON DELETE CASCADE,
    rel_path    TEXT NOT NULL,                 -- 볼륨 내 상대경로 (NFC 정규화)
    parent_id   INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    inode       INTEGER,                       -- Finder에서 이름을 바꿔도 추적
    area        INTEGER NOT NULL,              -- 0 작업대 · 1 내사진 · 2 공용 · 3 기타
    event_date  TEXT,                          -- 'YYYY-MM-DD' — 이벤트 폴더면
    event_name  TEXT,                          -- '거제통영 가족여행'
    file_count  INTEGER NOT NULL DEFAULT 0,    -- 캐시. 재귀 아님
    scanned_at  INTEGER,
    UNIQUE (volume_uuid, rel_path)
);
CREATE INDEX IF NOT EXISTS idx_folders_parent ON folders(parent_id);
CREATE INDEX IF NOT EXISTS idx_folders_area   ON folders(area);
CREATE INDEX IF NOT EXISTS idx_folders_inode  ON folders(volume_uuid, inode);
CREATE INDEX IF NOT EXISTS idx_folders_event  ON folders(event_date);

-- ---------------------------------------------------------------------------
-- 파일 — EXIF 인라인. 이 테이블 하나로 정렬·필터가 끝나야 한다
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS files (
    id          INTEGER PRIMARY KEY,
    folder_id   INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,                 -- NFC 정규화
    ext         TEXT,                          -- 소문자, 점 없음
    size        INTEGER NOT NULL,
    kind        INTEGER NOT NULL,              -- 0 사진 · 1 영상 · 2 RAW

    -- 시각 --------------------------------------------------------------
    -- taken_at은 절대 NULL이 아니다. 폴백 체인의 결과가 항상 들어간다:
    --   EXIF DateTimeOriginal → 파일명 파싱 → min-plausible(mtime, birthtime) → now
    -- 정렬은 오직 이 컬럼으로 한다 (COALESCE 금지 — 인덱스를 못 탄다)
    taken_at        INTEGER NOT NULL,
    taken_at_source INTEGER NOT NULL,          -- 0 exif · 1 파일명 · 2 파일시각 · 3 불명
    created_at      INTEGER,                   -- birthtime
    modified_at     INTEGER,                   -- mtime

    -- 동일성 ------------------------------------------------------------
    quick_hash  TEXT,                          -- xxHash64, 앞뒤 일부
    full_hash   TEXT,                          -- SHA-256, 전체
    image_hash  TEXT,                          -- SHA-256, JPEG 그림 데이터만(EXIF·XMP 등 머리 제외)
    phash       INTEGER,                       -- 64비트 지각 해시(i64로 담음) — 크기만 줄인 사본 찾기
    psig        BLOB,                          -- 16×16 회색조 256B — 그 짝이 정말 같은 그림인지 견주는 서명

    -- 이미지 속성 -------------------------------------------------------
    width       INTEGER,
    height      INTEGER,
    orientation INTEGER,
    duration_ms INTEGER,                       -- 영상

    -- 고르기 결과 — 그룹이 아니라 파일의 속성 -----------------------------
    culling_flag INTEGER NOT NULL DEFAULT 0,   -- 0 미판정 · 1 남김 · 2 제외
    rating       INTEGER NOT NULL DEFAULT 0,   -- 0~5
    favorite     INTEGER NOT NULL DEFAULT 0,
    comment      TEXT,

    -- EXIF 인라인 -------------------------------------------------------
    cam_make    TEXT,
    cam_model   TEXT,
    lens        TEXT,
    iso         INTEGER,
    aperture    REAL,
    shutter     TEXT,                          -- '1/250' 형태 그대로
    focal_mm    REAL,
    gps_lat     REAL,
    gps_lon     REAL,
    gps_alt     REAL,
    geo_name    TEXT,                          -- 역지오코딩 캐시 ('거제시')
    -- 지명 3단계 — 격자(0.01도)마다 한 번 물어 places 에 캐시한 값을 복사해 둔다.
    -- 파일에 두는 이유: 사이드바 묶음·필터가 조인 없이 곧바로 센다
    geo_country TEXT,                          -- '대한민국'
    geo_admin1  TEXT,                          -- 시도 — '경기도', '서울특별시'
    geo_admin2  TEXT,                          -- 시군구 — '수원시', '서초구'

    -- 품질 점수 (고르기용) ------------------------------------------------
    sharpness   REAL,
    exposure    REAL,

    -- AI — 나중에 채운다. 컬럼은 미리 둔다 (재인덱싱 방지) ------------------
    embedding   BLOB,

    -- 휴지통 -------------------------------------------------------------
    -- 행을 지우지 않고 표시만 한다. 그래야 되돌릴 때 평점·판정이 살아남는다.
    trashed_at   INTEGER,                     -- NULL이면 제자리에 있다
    trash_path   TEXT,                        -- 휴지통 안 경로 (라이브러리 기준)
    trash_batch  INTEGER REFERENCES batches(id) ON DELETE SET NULL,

    inode       INTEGER,
    scanned_at  INTEGER NOT NULL,
    UNIQUE (folder_id, name)
);

-- 정렬·필터가 전부 인덱스를 타야 한다
-- 목록 정렬과 커서가 쓰는 인덱스. id까지 넣는 이유는 같은 시각의 사진이
-- 여럿이기 때문이다. taken_at만 있으면 동점을 가르려고 SQLite가 임시 B-tree를
-- 만든다. 실측: 14만 행에서 순번 조회 80ms -> 3ms.
CREATE INDEX IF NOT EXISTS idx_files_taken_id  ON files(taken_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_files_taken     ON files(taken_at DESC);
CREATE INDEX IF NOT EXISTS idx_files_folder    ON files(folder_id, taken_at DESC);
-- idx_files_folder_live 도 db/upgrade.rs 가 만든다 (trashed_at 을 참조하므로 — 같은 이유)
CREATE INDEX IF NOT EXISTS idx_files_full_hash ON files(full_hash) WHERE full_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_files_quick     ON files(size, quick_hash);
-- idx_files_image_hash 도 db/upgrade.rs 가 만든다 (같은 이유 — 구버전 DB 엔 컬럼이 아직 없다)
CREATE INDEX IF NOT EXISTS idx_files_culling   ON files(culling_flag) WHERE culling_flag <> 0;
CREATE INDEX IF NOT EXISTS idx_files_rating    ON files(rating) WHERE rating > 0;
CREATE INDEX IF NOT EXISTS idx_files_kind      ON files(kind, taken_at DESC);
CREATE INDEX IF NOT EXISTS idx_files_gps       ON files(gps_lat, gps_lon) WHERE gps_lat IS NOT NULL;
-- idx_files_geo 는 db/upgrade.rs 가 만든다. 여기 두면 구버전 DB(칸이 아직 없다)에서
-- 이 배치가 통째로 실패해 앱이 아예 뜨지 않는다 — idx_files_trashed·image_hash 와 같은 이유

-- ---------------------------------------------------------------------------
-- 지명 캐시 — 좌표 격자(0.01도 ≈ 1.1km) 하나에 한 줄. 한 번 물어보면 영원히 쓴다.
-- 사진 5만 장이라도 서로 다른 격자는 천 개 남짓이다 (실측 2026-09-01: 1,143칸)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS places (
    cell    TEXT PRIMARY KEY,                  -- '37.28,127.05'
    country TEXT,
    admin1  TEXT,
    admin2  TEXT,
    name    TEXT,                              -- 표시용 — 가장 좁은 단계
    -- 'ok'        쓸 수 있는 이름이 있다
    -- 'none'      온라인 서버가 «그 자리엔 이름이 없다»고 확정했다 — 다시 묻지 않는다
    -- 'unresolved' 오프라인으로 안전하게 정하지 못했다 — 온라인으로 다시 물을 수 있다
    -- 셋을 섞으면 이름 없는 자리를 영영 다시 묻거나, 반대로 물어볼 것을 잃는다
    status  TEXT NOT NULL DEFAULT 'ok',
    -- 출처와 정밀도는 status 와 따로 둔다 — «어디서 왔나»와 «믿을 만한가»는 다른 축이다
    source  TEXT NOT NULL DEFAULT 'legacy',    -- legacy | offline_geonames | nominatim
    precision TEXT,                            -- approximate | boundary | remote
    distance_km REAL,                          -- 오프라인: 최근접 도시까지
    dataset_version TEXT,                      -- 오프라인 스냅샷 판
    provider TEXT,                             -- 온라인: 이 값을 준 서버 호스트
    resolved_at INTEGER,                       -- 이 값이 정해진 시각
    -- 온라인 «조회 결과»는 값의 출처와 다른 축이다. 서버가 이름을 못 찾았다고
    -- 해서 이미 가진 이름이 틀린 것은 아니므로, 값은 그대로 두고 여기에만 적는다.
    -- 이 칸이 없으면 같은 좌표를 볼 때마다 같은 서버에 되풀이해 묻게 된다.
    --   NULL       아직 안 물어봤다
    --   'success'  서버 답을 받아들여 값을 갱신했다
    --   'none'     그 자리에 이름이 없다고 했다
    --   'shallow'  기존보다 얕거나 국가 코드가 없는 부분 응답이었다
    --   'conflict' 국가가 내장 경계와 어긋났다
    online_outcome TEXT,
    online_provider TEXT,                      -- 그 답을 준 서버 호스트 (열쇠는 뺀다)
    online_checked_at INTEGER,                 -- 마지막으로 물어본 시각
    at      INTEGER NOT NULL                   -- 물어본 시각
);
CREATE INDEX IF NOT EXISTS idx_files_camera    ON files(cam_model) WHERE cam_model IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_files_inode     ON files(inode);
-- 정렬 기준마다 인덱스가 있어야 페이지마다 14만 행을 다시 줄 세우지 않는다.
-- 모두 (값, id) 쌍이다 — id가 없으면 같은 값에서 커서가 흔들린다.
CREATE INDEX IF NOT EXISTS idx_files_name      ON files(name, id);
CREATE INDEX IF NOT EXISTS idx_files_size_id   ON files(size, id);
CREATE INDEX IF NOT EXISTS idx_files_created   ON files(created_at, id);
CREATE INDEX IF NOT EXISTS idx_files_modified  ON files(modified_at, id);
-- idx_files_trashed는 db/upgrade.rs가 만든다. 여기 두면 구버전 DB에서
-- 컬럼이 아직 없는 채로 이 배치가 돌아 앱이 뜨지 않는다.

-- ---------------------------------------------------------------------------
-- 썸네일 — 무효화 키를 함께 저장한다. 원본이 바뀌면 다시 만든다
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS thumbs (
    file_id    INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    rel_path   TEXT,                           -- 캐시 루트 기준 상대경로
    src_size   INTEGER NOT NULL,               -- 만들 당시 원본 크기
    src_mtime  INTEGER NOT NULL,               -- 만들 당시 원본 수정시각
    width      INTEGER,
    height     INTEGER,
    state      INTEGER NOT NULL DEFAULT 0,     -- 0 대기 · 1 완료 · 2 실패
    error      TEXT,
    updated_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_thumbs_state ON thumbs(state) WHERE state <> 1;

-- ---------------------------------------------------------------------------
-- 태그
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tags (
    id    INTEGER PRIMARY KEY,
    name  TEXT NOT NULL UNIQUE,
    color TEXT
);
CREATE TABLE IF NOT EXISTS file_tags (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id)  ON DELETE CASCADE,
    PRIMARY KEY (file_id, tag_id)
);
CREATE INDEX IF NOT EXISTS idx_file_tags_tag ON file_tags(tag_id);

-- ---------------------------------------------------------------------------
-- 고르기 그룹 — 중복 · 잡동사니 · 같은 순간 · (나중) 시각적 유사
-- 판정 결과는 files.culling_flag에 남고, 그룹은 작업 단위일 뿐이다
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS groups (
    id         INTEGER PRIMARY KEY,
    kind       INTEGER NOT NULL,               -- 0 완전중복 · 1 잡동사니 · 2 같은순간 · 3 시각유사 · 4 줄인사본
    reason     TEXT,                           -- '스크린샷' 등 세부 사유
    size_bytes INTEGER NOT NULL DEFAULT 0,     -- 정리하면 확보되는 용량
    state      INTEGER NOT NULL DEFAULT 0,     -- 0 대기 · 1 처리됨 · 2 보류
    created_at INTEGER NOT NULL,
    done_at    INTEGER                         -- 처리된 시각 — «처리됨 보기»를 최근 순으로
);
CREATE TABLE IF NOT EXISTS group_members (
    group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    file_id  INTEGER NOT NULL REFERENCES files(id)  ON DELETE CASCADE,
    is_best  INTEGER NOT NULL DEFAULT 0,       -- 자동 선정된 대표
    score    REAL,
    PRIMARY KEY (group_id, file_id)
);
CREATE INDEX IF NOT EXISTS idx_groups_state   ON groups(kind, state);
CREATE INDEX IF NOT EXISTS idx_gmembers_file  ON group_members(file_id);

-- ---------------------------------------------------------------------------
-- NAS 상태 — 어떤 파일이 아직 안 올라갔는지
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS nas_state (
    file_id     INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    area        INTEGER NOT NULL,              -- 1 개인 · 2 공용
    remote_path TEXT,
    uploaded_at INTEGER,
    full_hash   TEXT                           -- 올린 시점의 해시. 바뀌면 재업로드
);
CREATE INDEX IF NOT EXISTS idx_nas_hash ON nas_state(full_hash);

-- ---------------------------------------------------------------------------
-- 작업 저널 — 이동·삭제·이름변경을 배치 단위로 되돌린다
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS batches (
    id         INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL,                  -- move | trash | rename | organize | upload
    label      TEXT,                           -- 사람이 읽을 설명
    item_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    undone_at  INTEGER
);
CREATE TABLE IF NOT EXISTS journal (
    id       INTEGER PRIMARY KEY,
    batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
    file_id  INTEGER,                          -- 삭제 후엔 끊길 수 있다
    op       TEXT NOT NULL,
    from_vol TEXT, from_path TEXT,
    to_vol   TEXT, to_path   TEXT,
    ok       INTEGER NOT NULL DEFAULT 1,
    error    TEXT
);
CREATE INDEX IF NOT EXISTS idx_journal_batch ON journal(batch_id);

-- ---------------------------------------------------------------------------
-- 스마트 앨범 — 조건을 저장해 두고 이름으로 부른다
--
-- 폴더가 "어디에 있나"라면 이것은 "어떤 것인가"다. 「별 4개 이상 영상」처럼
-- 자주 쓰는 조건에 이름을 붙여 둔다. 조건은 Filter를 그대로 JSON으로 담는다 —
-- 필터가 늘어나도 스키마를 바꾸지 않는다.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS smart_albums (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    filter     TEXT NOT NULL,               -- Filter를 직렬화한 JSON
    sort       TEXT,                        -- Sort를 직렬화한 JSON
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- ---------------------------------------------------------------------------
-- 설정
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT
);

-- ---------------------------------------------------------------------------
-- 나중에 — 얼굴/인물. 지금은 자리만 잡아둔다
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS persons (
    id         INTEGER PRIMARY KEY,
    name       TEXT,
    cover_file INTEGER REFERENCES files(id) ON DELETE SET NULL
);
CREATE TABLE IF NOT EXISTS faces (
    id        INTEGER PRIMARY KEY,
    file_id   INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    person_id INTEGER REFERENCES persons(id) ON DELETE SET NULL,
    bbox      TEXT,
    embedding BLOB
);
CREATE INDEX IF NOT EXISTS idx_faces_file   ON faces(file_id);
CREATE INDEX IF NOT EXISTS idx_faces_person ON faces(person_id);
