/** 되돌리기 단추 이름 — 작업 종류별로 «무엇이 어떻게 되는지»를 그대로 적는다.
 *  «되돌리기: 제외한 사진 휴지통으로»처럼 두 동사를 잇지 않는다 (사용자 지적 2026-08-30) */
export function undoLabel(kind: string, n: number): string {
  const c = n.toLocaleString();
  switch (kind) {
    case "trash":
      return `휴지통 보낸 ${c}장 되살리기`;
    case "restore":
      return `되살린 ${c}장 다시 휴지통으로`;
    case "move":
      return `정리 되돌리기 (${c}장)`;
    case "rename":
      return "이름 바꾸기 되돌리기";
    case "import":
      return `가져온 ${c}장 되돌리기`;
    default:
      return `되돌리기 (${c}장)`;
  }
}
