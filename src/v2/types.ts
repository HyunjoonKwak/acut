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
  /** 타일 배지·이름줄용 EXIF */
  iso: number | null;
  aperture: number | null;
  shutter: string | null;
  focal_mm: number | null;
  cam_model: string | null;
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
/** 역지오코딩 현황 — 처리 대기와 서버가 이름을 못 찾은 사진을 섞지 않는다. */
export type GeoStats = {
  with_gps: number;
  named: number;
  /** 이름은 붙었으나 온라인으로 더 정밀해질 수 있는 사진 (오프라인 결과) */
  approximate_files: number;
  /** 온라인 정밀 결과가 붙은 사진 */
  precise_files: number;
  /** 아직 이름이 없고 처리할 수 있는 사진 */
  pending_files: number;
  /** 서버가 «이름 없음»으로 확정한 사진 — 더 할 일이 없다 */
  unavailable_files: number;
  cells_left: number;
  /** 오프라인으로 새로 판정할 자리 — 서버가 필요 없다 */
  offline_cells_left: number;
  /** 이미 캐시에 이름이 있는데 아직 안 붙은 자리 — 옮겨 붙이기만 하면 된다 */
  cache_cells_left: number;
  /** 서버에만 물을 수 있는 자리 — 오프라인이 이미 포기했다 */
  network_cells_left: number;
  /** 서버에 물을 수 있는 자리 전부 — 못 채운 곳과 정밀 보강할 곳 */
  online_cells_left: number;
  endpoint_ready: boolean;
};
/** 라이브러리 하나의 휴지통 — 휴지통은 라이브러리마다 따로 있다 */
export type TrashByLib = {
  library_id: number;
  name: string;
  files: number;
  bytes: number;
};

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
  /** 부분 실패한 사진. 지원하지 않는 예전 응답에서는 없을 수 있다. */
  failed_ids?: number[];
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
  r.thumb && r.library_id !== null ? thumbUrlOf(r.library_id, r.thumb) : null;

/** 라이브러리 id + 캐시 상대경로 → thumb:// 주소. 화면 네 곳이 저마다 만들던 것을 하나로 */
export const thumbUrlOf = (libraryId: number, rel: string): string =>
  `thumb://localhost/${libraryId}/${rel.split("/").map(encodeURIComponent).join("/")}`;

/** 이미 «라이브러리 id/상대경로» 꼴로 온 것(사람 대표 얼굴 등) */
export const thumbUrlPath = (path: string): string =>
  `thumb://localhost/${path.split("/").map(encodeURIComponent).join("/")}`;
