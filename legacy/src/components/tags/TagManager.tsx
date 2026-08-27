import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { Tag as TagIcon, Plus, X, Check } from "lucide-react";
import type { Tag, MediaTag } from "@/types";

const TAG_CATEGORIES = ["general", "scene", "object", "person", "event", "custom"];

interface TagManagerProps {
  mediaIds?: string[];
  onClose?: () => void;
}

export function TagManager({ mediaIds, onClose }: TagManagerProps) {
  const { t } = useTranslation();
  const [tags, setTags] = useState<Tag[]>([]);
  const [mediaTags, setMediaTags] = useState<MediaTag[]>([]);
  const [loading, setLoading] = useState(false);
  const [newTagName, setNewTagName] = useState("");
  const [newTagCategory, setNewTagCategory] = useState("general");

  const loadTags = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<Tag[]>("get_tags");
      setTags(result);
    } catch {
      // Command not available yet
    }
    setLoading(false);
  }, []);

  const loadMediaTags = useCallback(async () => {
    if (!mediaIds || mediaIds.length === 0) return;
    try {
      const result = await invoke<MediaTag[]>("get_media_tags", {
        mediaIds,
      });
      setMediaTags(result);
    } catch {
      // Command not available yet
    }
  }, [mediaIds]);

  useEffect(() => {
    loadTags();
  }, [loadTags]);

  useEffect(() => {
    loadMediaTags();
  }, [loadMediaTags]);

  const handleCreateTag = async () => {
    if (!newTagName.trim()) return;
    try {
      await invoke("create_tag", {
        name: newTagName.trim(),
        category: newTagCategory,
      });
      setNewTagName("");
      loadTags();
    } catch {
      // Handle error
    }
  };

  const handleDeleteTag = async (tagId: string) => {
    try {
      await invoke("delete_tag", { tagId });
      loadTags();
    } catch {
      // Handle error
    }
  };

  const handleTagMedia = async (tagId: string) => {
    if (!mediaIds || mediaIds.length === 0) return;
    try {
      await invoke("tag_media", { mediaIds, tagId });
      loadMediaTags();
    } catch {
      // Handle error
    }
  };

  const handleUntagMedia = async (tagId: string) => {
    if (!mediaIds || mediaIds.length === 0) return;
    try {
      await invoke("untag_media", { mediaIds, tagId });
      loadMediaTags();
    } catch {
      // Handle error
    }
  };

  const isTagAssigned = (tagId: string) => {
    return mediaTags.some((mt) => mt.tag_id === tagId);
  };

  const groupedTags = tags.reduce<Record<string, Tag[]>>((acc, tag) => {
    if (!acc[tag.category]) acc[tag.category] = [];
    acc[tag.category].push(tag);
    return acc;
  }, {});

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-bg-secondary/50">
        <div className="flex items-center gap-2">
          <TagIcon size={18} className="text-accent" />
          <h2 className="text-sm font-semibold text-text-primary">
            {t("tags.title")}
          </h2>
          <span className="text-xs text-text-secondary">
            ({tags.length})
          </span>
        </div>
        {onClose && (
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-bg-secondary text-text-secondary hover:text-text-primary transition-colors"
          >
            <X size={16} />
          </button>
        )}
      </div>

      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {/* Create new tag */}
        <div className="bg-bg-secondary rounded-lg p-3 space-y-2">
          <h3 className="text-xs font-semibold text-text-primary">
            {t("tags.addTag")}
          </h3>
          <div className="flex gap-2">
            <input
              type="text"
              value={newTagName}
              onChange={(e) => setNewTagName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleCreateTag()}
              placeholder={t("tags.name")}
              className="flex-1 h-8 px-2.5 rounded-md text-xs bg-bg-primary border border-border text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent/50"
            />
            <select
              value={newTagCategory}
              onChange={(e) => setNewTagCategory(e.target.value)}
              className="h-8 px-2 rounded-md text-xs bg-bg-primary border border-border text-text-primary focus:outline-none focus:ring-1 focus:ring-accent/50"
            >
              {TAG_CATEGORIES.map((cat) => (
                <option key={cat} value={cat}>
                  {cat}
                </option>
              ))}
            </select>
            <button
              onClick={handleCreateTag}
              disabled={!newTagName.trim()}
              className="h-8 px-3 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover transition-colors disabled:opacity-50"
            >
              <Plus size={14} />
            </button>
          </div>
        </div>

        {/* Media tagging section */}
        {mediaIds && mediaIds.length > 0 && (
          <div className="bg-bg-secondary rounded-lg p-3 space-y-2">
            <h3 className="text-xs font-semibold text-text-primary">
              {t("tags.assignTag")} ({mediaIds.length})
            </h3>
            <div className="flex flex-wrap gap-1.5">
              {tags.map((tag) => {
                const assigned = isTagAssigned(tag.id);
                return (
                  <button
                    key={tag.id}
                    onClick={() =>
                      assigned
                        ? handleUntagMedia(tag.id)
                        : handleTagMedia(tag.id)
                    }
                    className={`flex items-center gap-1 px-2.5 py-1 rounded-full text-[11px] font-medium transition-all ${
                      assigned
                        ? "bg-accent text-white"
                        : "bg-bg-primary border border-border text-text-secondary hover:border-accent hover:text-accent"
                    }`}
                  >
                    {assigned && <Check size={10} />}
                    {tag.name}
                  </button>
                );
              })}
            </div>
          </div>
        )}

        {/* Tag list grouped by category */}
        {loading ? (
          <div className="flex justify-center py-8">
            <div className="w-6 h-6 border-2 border-accent border-t-transparent rounded-full animate-spin" />
          </div>
        ) : tags.length === 0 ? (
          <div className="text-center py-8">
            <TagIcon size={32} className="mx-auto mb-2 text-text-secondary/30" />
            <p className="text-xs text-text-secondary">{t("tags.noTags")}</p>
          </div>
        ) : (
          Object.entries(groupedTags).map(([category, categoryTags]) => (
            <div key={category} className="space-y-1.5">
              <h3 className="text-[10px] font-semibold text-text-secondary uppercase tracking-wider px-1">
                {category}
              </h3>
              <div className="flex flex-wrap gap-1.5">
                {categoryTags.map((tag) => (
                  <div
                    key={tag.id}
                    className="group flex items-center gap-1 px-2.5 py-1 rounded-full bg-bg-secondary border border-border text-[11px] text-text-primary"
                  >
                    <span>{tag.name}</span>
                    <button
                      onClick={() => handleDeleteTag(tag.id)}
                      className="opacity-0 group-hover:opacity-100 text-danger hover:text-danger transition-opacity"
                    >
                      <X size={10} />
                    </button>
                  </div>
                ))}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
