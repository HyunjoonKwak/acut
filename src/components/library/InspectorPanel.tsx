import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import { toast } from "@/stores/toastStore";
import { Thumbnail } from "@/components/gallery/GalleryGrid";
import { formatFileSize, formatDate } from "@/utils/format";
import { X, Plus } from "lucide-react";
import type { Album, Tag } from "@/types";

/**
 * 인스펙터 — 선택한 사진의 정보·태그·앨범·코멘트를
 * 화면 이동 없이 한 패널에서 처리한다.
 */
export function InspectorPanel() {
  const { t } = useTranslation();
  const media = useAppStore((s) => s.inspectorMedia);
  const setInspectorMedia = useAppStore((s) => s.setInspectorMedia);
  const comments = useAppStore((s) => s.comments);
  const setComment = useAppStore((s) => s.setComment);

  const [mediaTags, setMediaTags] = useState<Tag[]>([]);
  const [allTags, setAllTags] = useState<Tag[]>([]);
  const [albums, setAlbums] = useState<Album[]>([]);
  const [mediaAlbums, setMediaAlbums] = useState<Album[]>([]);
  const [tagInput, setTagInput] = useState("");
  const [albumChoice, setAlbumChoice] = useState("");
  const [commentDraft, setCommentDraft] = useState("");
  const [saving, setSaving] = useState(false);

  const mediaId = media?.id ?? null;

  useEffect(() => {
    if (!mediaId) return;
    setCommentDraft(useAppStore.getState().comments.get(mediaId) ?? "");
    setTagInput("");
    let cancelled = false;
    (async () => {
      try {
        const tags = await invoke<Tag[]>("get_media_tags", { mediaId });
        if (!cancelled) setMediaTags(tags);
      } catch {
        if (!cancelled) setMediaTags([]);
      }
      try {
        const tags = await invoke<Tag[]>("get_tags");
        if (!cancelled) setAllTags(tags);
      } catch {
        // ignore
      }
      try {
        const list = await invoke<Album[]>("get_albums");
        if (!cancelled) setAlbums(list);
      } catch {
        // ignore
      }
      try {
        const list = await invoke<Album[]>("get_media_albums", { mediaId });
        if (!cancelled) setMediaAlbums(list);
      } catch {
        if (!cancelled) setMediaAlbums([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [mediaId]);

  if (!media) return null;

  const folder = media.file_path.split("/").slice(0, -1).join("/") || "/";

  const handleAddTag = async () => {
    const name = tagInput.trim();
    if (!name) return;
    try {
      let tag = allTags.find(
        (t) => t.name.toLowerCase() === name.toLowerCase(),
      );
      if (!tag) {
        tag = await invoke<Tag>("create_tag", { name, category: "user" });
        setAllTags((prev) => [...prev, tag!]);
      }
      if (!mediaTags.some((mt) => mt.id === tag!.id)) {
        await invoke("tag_media", { mediaId: media.id, tagId: tag.id });
        setMediaTags((prev) => [...prev, tag!]);
      }
      setTagInput("");
    } catch (e) {
      toast.error(String(e));
    }
  };

  const handleRemoveTag = async (tagId: string) => {
    try {
      await invoke("untag_media", { mediaId: media.id, tagId });
      setMediaTags((prev) => prev.filter((t) => t.id !== tagId));
    } catch (e) {
      toast.error(String(e));
    }
  };

  const handleAddToAlbum = async () => {
    if (!albumChoice) return;
    try {
      await invoke("add_media_to_album", {
        albumId: albumChoice,
        mediaIds: [media.id],
      });
      const album = albums.find((a) => a.id === albumChoice);
      toast.success(
        t("inspector.addedToAlbum", { name: album?.name ?? "" }),
      );
      const list = await invoke<Album[]>("get_media_albums", {
        mediaId: media.id,
      });
      setMediaAlbums(list);
      setAlbumChoice("");
    } catch (e) {
      toast.error(String(e));
    }
  };

  const handleRemoveFromAlbum = async (albumId: string) => {
    try {
      await invoke("remove_media_from_album", {
        albumId,
        mediaIds: [media.id],
      });
      setMediaAlbums((prev) => prev.filter((a) => a.id !== albumId));
    } catch (e) {
      toast.error(String(e));
    }
  };

  const handleSaveComment = async () => {
    setSaving(true);
    try {
      await invoke("set_media_comment", {
        mediaId: media.id,
        comment: commentDraft,
      });
      setComment(media.id, commentDraft);
      toast.success(t("inspector.commentSaved"));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setSaving(false);
    }
  };

  const savedComment = comments.get(media.id) ?? "";
  const commentDirty = commentDraft.trim() !== savedComment;

  return (
    <aside className="w-64 shrink-0 border-l border-border bg-bg-secondary/40 flex flex-col overflow-y-auto">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border sticky top-0 bg-bg-secondary/90 backdrop-blur z-10">
        <span className="text-[11px] font-semibold text-text-primary uppercase tracking-wider">
          {t("inspector.title")}
        </span>
        <button
          onClick={() => setInspectorMedia(null)}
          className="p-1 rounded text-text-secondary hover:text-text-primary hover:bg-bg-primary transition-colors"
          title={t("inspector.close")}
        >
          <X size={13} />
        </button>
      </div>

      <div className="p-3 flex flex-col gap-4">
        {/* Preview */}
        <div className="rounded-lg overflow-hidden border border-border bg-bg-secondary aspect-[4/3] flex items-center justify-center">
          <Thumbnail file={media} />
        </div>

        {/* Meta */}
        <div>
          <p className="text-[12px] font-semibold text-text-primary break-all">
            {media.file_name}
          </p>
          <div className="mt-2 flex flex-col gap-1 text-[10.5px] text-text-secondary">
            <MetaRow label={t("inspector.size")} value={formatFileSize(media.file_size)} />
            {media.width != null && media.height != null && (
              <MetaRow
                label={t("inspector.dimensions")}
                value={`${media.width}×${media.height}`}
              />
            )}
            {media.date_taken && (
              <MetaRow
                label={t("inspector.dateTaken")}
                value={formatDate(media.date_taken)}
              />
            )}
            <MetaRow label={t("inspector.type")} value={media.media_type} />
            <MetaRow label={t("inspector.folder")} value={folder} breakAll />
          </div>
        </div>

        {/* Tags */}
        <div>
          <p className="text-[10px] font-semibold text-text-secondary uppercase tracking-wider mb-1.5">
            {t("inspector.tags")}
          </p>
          <div className="flex flex-wrap gap-1.5">
            {mediaTags.map((tag) => (
              <span
                key={tag.id}
                className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-accent/10 text-accent text-[10px]"
              >
                #{tag.name}
                <button
                  onClick={() => handleRemoveTag(tag.id)}
                  className="hover:text-danger transition-colors"
                  title={t("inspector.removeTag")}
                >
                  <X size={9} />
                </button>
              </span>
            ))}
          </div>
          <div className="mt-2 flex gap-1">
            <input
              type="text"
              value={tagInput}
              onChange={(e) => setTagInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleAddTag();
              }}
              placeholder={t("inspector.addTagPlaceholder")}
              list="inspector-tag-options"
              className="flex-1 min-w-0 h-6 px-2 rounded text-[10px] bg-bg-primary border border-border text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:border-accent/50"
            />
            <datalist id="inspector-tag-options">
              {allTags.map((tag) => (
                <option key={tag.id} value={tag.name} />
              ))}
            </datalist>
            <button
              onClick={handleAddTag}
              className="h-6 px-2 rounded bg-accent/10 text-accent hover:bg-accent/20 transition-colors"
              title={t("inspector.addTag")}
            >
              <Plus size={11} />
            </button>
          </div>
        </div>

        {/* Albums */}
        <div>
          <p className="text-[10px] font-semibold text-text-secondary uppercase tracking-wider mb-1.5">
            {t("inspector.albums")}
          </p>
          {mediaAlbums.length > 0 && (
            <div className="flex flex-wrap gap-1.5 mb-2">
              {mediaAlbums.map((album) => (
                <span
                  key={album.id}
                  className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-accent/10 text-accent text-[10px]"
                >
                  {album.name}
                  <button
                    onClick={() => handleRemoveFromAlbum(album.id)}
                    className="hover:text-danger transition-colors"
                    title={t("inspector.removeFromAlbum")}
                  >
                    <X size={9} />
                  </button>
                </span>
              ))}
            </div>
          )}
          <div className="flex gap-1">
            <select
              value={albumChoice}
              onChange={(e) => setAlbumChoice(e.target.value)}
              className="flex-1 min-w-0 h-6 px-1.5 rounded text-[10px] bg-bg-primary border border-border text-text-primary focus:outline-none focus:border-accent/50"
            >
              <option value="">{t("inspector.chooseAlbum")}</option>
              {albums.map((album) => (
                <option key={album.id} value={album.id}>
                  {album.name}
                </option>
              ))}
            </select>
            <button
              onClick={handleAddToAlbum}
              disabled={!albumChoice}
              className="h-6 px-2 rounded bg-accent/10 text-accent hover:bg-accent/20 transition-colors disabled:opacity-40"
              title={t("inspector.addToAlbum")}
            >
              <Plus size={11} />
            </button>
          </div>
        </div>

        {/* Comment */}
        <div>
          <p className="text-[10px] font-semibold text-text-secondary uppercase tracking-wider mb-1.5">
            {t("inspector.comment")}
          </p>
          <textarea
            value={commentDraft}
            onChange={(e) => setCommentDraft(e.target.value)}
            placeholder={t("inspector.commentPlaceholder")}
            rows={3}
            className="w-full px-2 py-1.5 rounded text-[11px] bg-bg-primary border border-border text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:border-accent/50 resize-none"
          />
          <button
            onClick={handleSaveComment}
            disabled={!commentDirty || saving}
            className="mt-1 w-full h-6 rounded text-[10px] font-semibold bg-accent text-white hover:bg-accent/90 transition-colors disabled:opacity-40"
          >
            {saving ? t("inspector.saving") : t("inspector.saveComment")}
          </button>
        </div>
      </div>
    </aside>
  );
}

function MetaRow({
  label,
  value,
  breakAll,
}: {
  label: string;
  value: string;
  breakAll?: boolean;
}) {
  return (
    <div className="flex gap-2">
      <span className="w-12 shrink-0 text-text-secondary/70">{label}</span>
      <span className={`text-text-primary/90 ${breakAll ? "break-all" : ""}`}>
        {value}
      </span>
    </div>
  );
}
