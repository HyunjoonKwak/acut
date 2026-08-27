/** 이름에 못 쓰는 것. 슬래시는 폴더 구분자다. 문제가 없으면 null. */
export const badName = (n: string): string | null => {
  const t = n.trim();
  if (!t) return "이름이 비어 있습니다";
  if (t.includes("/")) return "이름에 /는 쓸 수 없습니다";
  if (t === "." || t === "..") return "그 이름은 쓸 수 없습니다";
  return null;
};
