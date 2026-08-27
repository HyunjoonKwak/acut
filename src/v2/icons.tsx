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

/** 설정 — 톱니 */
export const IconSettings = (p: P) => (
  <S {...p}>
    <circle cx="12" cy="12" r="3.1" />
    <path d="M12 2.8v2.4M12 18.8v2.4M4.5 12H2.1M21.9 12h-2.4M6.7 6.7L5 5M19 19l-1.7-1.7M17.3 6.7L19 5M5 19l1.7-1.7" />
  </S>
);
