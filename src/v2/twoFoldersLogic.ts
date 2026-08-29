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
  /** 이미 제외 표시된 파일 수 — 한쪽이 전부면 «처리됨» */
  flagged_a: number;
  flagged_b: number;
  /** B 쪽 사진이 전부 A 쪽(하위 폴더 포함)에 있다 — B 를 지워도 잃는 것이 없다 */
  b_in_a: boolean;
  /** A 쪽 사진이 전부 B 쪽에 있다 */
  a_in_b: boolean;
  /** 이 줄이 대표하는 폴더 행들(하위 폴더 포함) */
  a_ids: number[];
  b_ids: number[];
};

/** 이 줄에서 지워도 되는 쪽 — «b» 면 B 를 지워도 잃는 것이 없다 */
export function droppable(r: PairRow, side: "a" | "b"): boolean {
  if (!r.a || !r.b) return false;
  return side === "b" ? r.b_in_a : r.a_in_b;
}

/** 줄의 상태 문구 */
export function verdict(r: PairRow): { kind: "same" | "b_in_a" | "a_in_b" | "partial" | "a_only" | "b_only"; text: string } {
  if (!r.a) return { kind: "b_only", text: "B에만 있음" };
  if (!r.b) return { kind: "a_only", text: "A에만 있음" };
  if (r.same) return { kind: "same", text: "✓ 똑같음" };
  if (r.b_in_a) return { kind: "b_in_a", text: "B쪽이 A에 다 있음 — B 지워도 됨" };
  if (r.a_in_b) return { kind: "a_in_b", text: "A쪽이 B에 다 있음 — A 지워도 됨" };
  return { kind: "partial", text: `${r.common.toLocaleString()}장 똑같음` };
}

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
  // «똑같음»뿐 아니라 «한쪽이 다른 쪽에 다 있음» 짝도 처리될 수 있다 — same 만 보면
  // 표시가 다 붙은 짝이 «B쪽 전부 (41짝)»에 그대로 남는다 (실측 2026-08-30)
  if (!r.a || !r.b) return null;
  if (!(r.same || r.b_in_a || r.a_in_b)) return null;
  if (r.files_a > 0 && r.flagged_a >= r.files_a) return "a";
  if (r.files_b > 0 && r.flagged_b >= r.files_b) return "b";
  return null;
}

