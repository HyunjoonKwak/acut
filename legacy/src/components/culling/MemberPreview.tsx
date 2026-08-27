import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { thumbSrc } from "@/utils/media";
import { ImageOff } from "lucide-react";

/**
 * 고르기 공용 고해상 미리보기.
 * 썸네일을 즉시 보여주고, 1024px 프리뷰(base64)가 로드되면 교체한다 —
 * Quick Look 없이 사진을 보면서 고를 수 있게 하는 핵심 컴포넌트.
 */

// Small module-level cache so moving between groups/members feels instant
const previewCache = new Map<string, string>();
const CACHE_MAX = 48;

function cachePut(key: string, value: string) {
  if (previewCache.size >= CACHE_MAX) {
    const oldest = previewCache.keys().next().value;
    if (oldest !== undefined) previewCache.delete(oldest);
  }
  previewCache.set(key, value);
}

export function MemberPreview({
  filePath,
  mediaType = "image",
  thumbnail,
  alt,
  fit = "cover",
  dimmed = false,
  className = "",
  onClick,
  children,
}: {
  filePath: string;
  mediaType?: string;
  thumbnail?: string | null;
  alt: string;
  /** cover = 카드 채움(연사 그리드), contain = 원본 비율(대형 패널) */
  fit?: "cover" | "contain";
  dimmed?: boolean;
  className?: string;
  onClick?: () => void;
  children?: React.ReactNode;
}) {
  const [state, setState] = useState<{
    path: string;
    preview: string | null;
    failed: boolean;
  }>(() => ({
    path: filePath,
    preview: previewCache.get(filePath) ?? null,
    failed: false,
  }));

  // Prop change → reset during render (no cascading effect render)
  if (state.path !== filePath) {
    setState({
      path: filePath,
      preview: previewCache.get(filePath) ?? null,
      failed: false,
    });
  }

  const needsLoad = state.preview === null && !state.failed;

  useEffect(() => {
    if (!needsLoad) return;
    let cancelled = false;
    const cmd =
      mediaType === "video" ? "get_preview_video_frame" : "get_preview_image";
    invoke<string>(cmd, { filePath })
      .then((b64) => {
        cachePut(filePath, b64);
        if (!cancelled) {
          setState((s) =>
            s.path === filePath ? { ...s, preview: b64 } : s,
          );
        }
      })
      .catch(() => {
        if (!cancelled) {
          setState((s) => (s.path === filePath ? { ...s, failed: true } : s));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [filePath, mediaType, needsLoad]);

  const preview = state.preview;
  const failed = state.failed;

  const thumb = thumbSrc(thumbnail ?? null);
  const imgClass =
    fit === "cover"
      ? "w-full h-full object-cover"
      : "max-h-full max-w-full object-contain";
  const dimClass = dimmed ? "opacity-60 group-hover:opacity-90" : "";

  return (
    <div
      className={`relative flex items-center justify-center overflow-hidden ${
        onClick ? "cursor-zoom-in" : ""
      } ${className}`}
      onClick={onClick}
    >
      {preview ? (
        <img
          src={`data:image/jpeg;base64,${preview}`}
          alt={alt}
          className={`${imgClass} transition-opacity duration-150 ${dimClass}`}
        />
      ) : thumb ? (
        <img
          src={thumb}
          alt={alt}
          className={`${imgClass} ${dimClass} ${failed ? "" : "blur-[2px]"}`}
        />
      ) : failed ? (
        <ImageOff size={20} className="text-white/15" />
      ) : (
        <div className="w-5 h-5 border-2 border-white/15 border-t-white/50 rounded-full animate-spin" />
      )}
      {children}
    </div>
  );
}
