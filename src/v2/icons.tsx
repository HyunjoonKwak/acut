/**
 * 레일과 툴바가 쓰는 그림들.
 *
 * 이모지를 쓰다 그만뒀다. 시스템이 주는 대로라 크기·색·굵기가 제각각이고
 * (🗀는 가늘고 📷는 통짜 컬러다) 고른 갈래를 강조할 방법도 없다. 선으로
 * 그린 아이콘은 `currentColor`를 따라와 글자색 하나로 상태가 표현된다.
 *
 * 모두 24×24 격자, 선 굵기 1.6. 획을 더 넣지 말 것 — 20px 아래로 줄면
 * 뭉개진다.
 */

type P = { className?: string };

const S = ({ children, className }: P & { children: React.ReactNode }) => (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={1.6}
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
    aria-hidden="true"
  >
    {children}
  </svg>
);

/** 모든 사진 — 격자 */
export const IconAll = (p: P) => (
  <S {...p}>
    <rect x="3.5" y="3.5" width="7" height="7" rx="1.4" />
    <rect x="13.5" y="3.5" width="7" height="7" rx="1.4" />
    <rect x="3.5" y="13.5" width="7" height="7" rx="1.4" />
    <rect x="13.5" y="13.5" width="7" height="7" rx="1.4" />
  </S>
);

/** 앨범 — 폴더 */
export const IconAlbum = (p: P) => (
  <S {...p}>
    <path d="M3.5 6.5a1.5 1.5 0 0 1 1.5-1.5h3.6c.5 0 .96.24 1.24.65l.92 1.35H19a1.5 1.5 0 0 1 1.5 1.5v9.5a1.5 1.5 0 0 1-1.5 1.5H5a1.5 1.5 0 0 1-1.5-1.5z" />
  </S>
);

/** 스마트 앨범 — 조건이 알아서 모아 준다는 뜻의 반짝임 */
export const IconSmart = (p: P) => (
  <S {...p}>
    <path d="M12 3.5l1.7 4.3 4.3 1.7-4.3 1.7L12 15.5l-1.7-4.3L6 9.5l4.3-1.7z" />
    <path d="M18 16l.7 1.8 1.8.7-1.8.7-.7 1.8-.7-1.8-1.8-.7 1.8-.7z" />
  </S>
);

/** 검색 — 돋보기 */
export const IconSearch = (p: P) => (
  <S {...p}>
    <circle cx="11" cy="11" r="6.5" />
    <path d="M15.8 15.8L20.5 20.5" />
  </S>
);

/** 태그 — 꼬리표 */
export const IconTag = (p: P) => (
  <S {...p}>
    <path d="M11.6 3.5H19a1.5 1.5 0 0 1 1.5 1.5v7.4a1.5 1.5 0 0 1-.44 1.06l-6.6 6.6a1.5 1.5 0 0 1-2.12 0l-7.4-7.4a1.5 1.5 0 0 1 0-2.12l6.6-6.6a1.5 1.5 0 0 1 1.06-.44z" />
    <circle cx="16" cy="8" r="1.4" />
  </S>
);

/** 사람 — 얼굴 둘 */
export const IconPeople = (p: P) => (
  <S {...p}>
    <circle cx="9" cy="8.5" r="3.2" />
    <path d="M3.5 19c0-3.2 2.5-5.3 5.5-5.3s5.5 2.1 5.5 5.3" />
    <circle cx="16.5" cy="9.5" r="2.5" />
    <path d="M15.5 13.6c2.9.2 5 2.1 5 5" />
  </S>
);

/** 달력 */
export const IconCalendar = (p: P) => (
  <S {...p}>
    <rect x="3.5" y="5.5" width="17" height="15" rx="1.8" />
    <path d="M3.5 10.5h17M8 3.5v4M16 3.5v4" />
  </S>
);

/** 위치 — 핀 */
export const IconLocation = (p: P) => (
  <S {...p}>
    <path d="M12 20.5s6.5-5.6 6.5-10.2a6.5 6.5 0 1 0-13 0C5.5 14.9 12 20.5 12 20.5z" />
    <circle cx="12" cy="10" r="2.4" />
  </S>
);

/** 카메라 */
export const IconCamera = (p: P) => (
  <S {...p}>
    <path d="M3.5 8.5a1.5 1.5 0 0 1 1.5-1.5h2.6l1.3-2.2h6.2L16.4 7H19a1.5 1.5 0 0 1 1.5 1.5V18a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 18z" />
    <circle cx="12" cy="13" r="3.6" />
  </S>
);

/** 휴지통 */
export const IconTrash = (p: P) => (
  <S {...p}>
    <path d="M4.5 6.5h15M9.5 6.5V4.8c0-.7.6-1.3 1.3-1.3h2.4c.7 0 1.3.6 1.3 1.3v1.7" />
    <path d="M6.5 6.5l.9 12.2c.05.73.66 1.3 1.4 1.3h6.4c.74 0 1.35-.57 1.4-1.3l.9-12.2" />
    <path d="M10.5 10.5v6M13.5 10.5v6" />
  </S>
);

// ── 보기 방식 ────────────────────────────────────────────────────────
// 그림이 곧 격자 모양이다. 「카드」라는 낱말보다 네모 밑의 글줄 두 개가
// 무엇이 달라지는지 빨리 말해 준다.

/** 카드 보기 — 사진 아래에 이름이 붙는 네모 칸 */
export const IconCard = (p: P) => (
  <S {...p}>
    <rect x="3.5" y="3.5" width="7.6" height="7.6" rx="1.4" />
    <rect x="12.9" y="3.5" width="7.6" height="7.6" rx="1.4" />
    <rect x="3.5" y="12.9" width="7.6" height="7.6" rx="1.4" />
    <rect x="12.9" y="12.9" width="7.6" height="7.6" rx="1.4" />
  </S>
);

/** 타일 보기 — 빈틈없는 격자 */
export const IconTile = (p: P) => (
  <S {...p}>
    <rect x="3.5" y="3.5" width="5.6" height="5.6" rx="1" />
    <rect x="9.2" y="3.5" width="5.6" height="5.6" rx="1" />
    <rect x="14.9" y="3.5" width="5.6" height="5.6" rx="1" />
    <rect x="3.5" y="9.2" width="5.6" height="5.6" rx="1" />
    <rect x="9.2" y="9.2" width="5.6" height="5.6" rx="1" />
    <rect x="14.9" y="9.2" width="5.6" height="5.6" rx="1" />
    <rect x="3.5" y="14.9" width="5.6" height="5.6" rx="1" />
    <rect x="9.2" y="14.9" width="5.6" height="5.6" rx="1" />
    <rect x="14.9" y="14.9" width="5.6" height="5.6" rx="1" />
  </S>
);

/** 양쪽 맞춤 — 줄마다 폭이 다르고 오른쪽 끝이 맞는다 */
export const IconJustified = (p: P) => (
  <S {...p}>
    <rect x="3.5" y="5" width="10.5" height="6" rx="1.2" />
    <rect x="15.5" y="5" width="5" height="6" rx="1.2" />
    <rect x="3.5" y="13" width="5" height="6" rx="1.2" />
    <rect x="10" y="13" width="10.5" height="6" rx="1.2" />
  </S>
);

/** 이름·크기 표시 — 그림 밑의 글줄 */
export const IconCaption = (p: P) => (
  <S {...p}>
    <path d="M3.5 6h17M3.5 11h17M3.5 16h11M3.5 21h7" />
  </S>
);

/** 필름스트림 — 아래에 깔리는 띠 */
export const IconFilmstrip = (p: P) => (
  <S {...p}>
    <rect x="3.5" y="3.5" width="17" height="10" rx="1.4" />
    <rect x="3.5" y="16.5" width="4.6" height="4" rx="0.8" />
    <rect x="9.7" y="16.5" width="4.6" height="4" rx="0.8" />
    <rect x="15.9" y="16.5" width="4.6" height="4" rx="0.8" />
  </S>
);

/** 메이슨리 — 열 폭은 같고 높이가 제각각, 벽돌 쌓듯 */
export const IconMasonry = (p: P) => (
  <S {...p}>
    <rect x="3.5" y="3.5" width="7.5" height="9" rx="1.2" />
    <rect x="13" y="3.5" width="7.5" height="5.5" rx="1.2" />
    <rect x="3.5" y="14.5" width="7.5" height="6" rx="1.2" />
    <rect x="13" y="11" width="7.5" height="9.5" rx="1.2" />
  </S>
);

/** 설정 — 톱니 */
export const IconSettings = (p: P) => (
  <S {...p}>
    {/* 톱니 — 이가 여덟. 원에 선을 두르면 햇살로 읽힌다. */}
    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.39a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
    <circle cx="12" cy="12" r="3" />
  </S>
);
