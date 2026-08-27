/**
 * 백엔드와 주고받는 모양들. Rust 쪽 struct와 이름·형이 같아야 한다.
 */

export type FileRow = {
  id: number;
  name: string;
  taken_at: number;
  taken_at_source: number;
  kind: number;
  size: number;
  width: number | null;
  height: number | null;
  rating: number;
  culling_flag: number;
  favorite: boolean;
  duration_ms: number | null;
  /** 묶기를 켰을 때의 그룹 값. 서버가 행마다 붙여 준다. */
  group: string | null;
  /** 어느 라이브러리 소속인가. 썸네일 주소를 만들 때 쓴다 */
  library_id: number | null;
  /** 캐시 루트 기준 상대경로. null이면 아직 생성 전 */
  thumb: string | null;
};

export type Cursor = { num: number | null; text: string | null; id: number };
export type Page = { rows: FileRow[]; next: Cursor | null };

/** 등록한 사진 폴더. 여러 개, 서로 다른 디스크에 있어도 된다 */
export type Library = {
  id: number;
  volume_uuid: string;
  volume_name: string;
  rel_path: string;
  name: string;
  area: number;
  /** 지금 그 디스크가 꽂혀 있는가 */
  online: boolean;
  dir: string | null;
  file_count: number;
};

export type Stats = {
  files: number;
  bytes: number;
  thumbs_done: number;
  thumbs_pending: number;
};

/** 캐시 용량 — 디스크를 훑어야 해서 자주 부르지 않는다 */
export type CacheUsage = { bytes: number; files: number };
export type Counted = { files: number; bytes: number };

export type Batch = {
  id: number;
  kind: string;
  label: string | null;
  item_count: number;
  created_at: number;
  undone_at: number | null;
};

export type Outcome = {
  batch_id: number;
  moved: number;
  failed: number;
  bytes: number;
  first_error: string | null;
};

export type Bucket = {
  year: number;
  month: number;
  count: number;
  top: number;
};

/** 사이드바 트리 한 줄. 중간 마디는 DB 행이 없어 id가 null이다. */
export type FolderRow = {
  id: number | null;
  /** 이 줄이 속한 라이브러리 */
  library_id: number;
  /** 라이브러리 루트 기준 — 접기의 열쇠 */
  path: string;
  /** 볼륨 기준 — 필터로 보낸다 */
  rel_path: string;
  name: string;
  depth: number;
  file_count: number;
  has_children: boolean;
  /** 라이브러리 자신인가 (트리의 뿌리) */
  is_library: boolean;
};

/** 판정 하나를 바꿀 때 넘기는 것. 없는 열쇠는 안 건드린다. */
export type Mark = {
  rating?: number;
  cullingFlag?: number;
  favorite?: boolean;
};

/** 한 번에 읽어 오는 장수 */
export const PAGE = 300;

/**
 * 전용 thumb:// 프로토콜 — 캐시 폴더만 서빙한다 (api/thumb_protocol.rs).
 * 캐시가 라이브러리마다 따로 있어 주소 앞에 라이브러리 id가 붙는다.
 */
export const thumbUrl = (r: {
  thumb: string | null;
  library_id: number | null;
}): string | null =>
  r.thumb && r.library_id !== null
    ? `thumb://localhost/${r.library_id}/${r.thumb
        .split("/")
        .map(encodeURIComponent)
        .join("/")}`
    : null;
