/**
 * 두 폴더 비교의 순수한 부분 — 화면 없이 시험한다.
 */

export type FolderIn = {
  folder_id: number;
  library_id: number;
  library: string;
  folder: string;
  area: number;
};

export type PairRow = {
  a: FolderIn | null;
  b: FolderIn | null;
  files_a: number;
  files_b: number;
  same: boolean;
  common: number;
  bytes: number;
  /** 이미 지우기 표시된 파일 수 — 한쪽이 전부면 «처리됨» */
  flagged_a: number;
  flagged_b: number;
};

/** Finder 로 고른 폴더가 라이브러리 안의 어느 폴더인가 */
export type FolderHit = {
  id: number | null;
  library_id: number;
  library: string;
  path: string;
  volume_uuid: string;
  vol_rel: string;
  abs: string;
  file_count: number;
};

/** 두 뿌리가 서로를 품는가 — 같은 폴더가 양쪽에 들어 제 짝이 되는 길 */
export function overlaps(a: FolderHit, b: FolderHit): boolean {
  if (a.volume_uuid !== b.volume_uuid) return false;
  const under = (root: string, p: string) => root === "" || p === root || p.startsWith(root + "/");
  return under(a.vol_rel, b.vol_rel) || under(b.vol_rel, a.vol_rel);
}

/** 같은 짝인데 한쪽이 이미 전부 지우기 표시됐다 — 다시 누르면 뒤집히니 단추를 감춘다 */
export function doneSide(r: PairRow): "a" | "b" | null {
  if (!r.same || !r.a || !r.b) return null;
  if (r.files_a > 0 && r.flagged_a >= r.files_a) return "a";
  if (r.files_b > 0 && r.flagged_b >= r.files_b) return "b";
  return null;
}

