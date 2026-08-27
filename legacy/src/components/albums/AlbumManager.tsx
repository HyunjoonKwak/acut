import { useState, useEffect, useCallback } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { FolderHeart, Plus, Trash2, ArrowLeft, ImageOff } from "lucide-react";
import type { Album, MediaFile } from "@/types";

export function AlbumManager() {
  const { t } = useTranslation();
  const [albums, setAlbums] = useState<Album[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedAlbum, setSelectedAlbum] = useState<Album | null>(null);
  const [albumMedia, setAlbumMedia] = useState<MediaFile[]>([]);
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState("");
  const [newDesc, setNewDesc] = useState("");

  const loadAlbums = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<Album[]>("get_albums");
      setAlbums(result);
    } catch {
      // Command not available yet
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    loadAlbums();
  }, [loadAlbums]);

  const loadAlbumMedia = useCallback(async (albumId: string) => {
    try {
      const result = await invoke<MediaFile[]>("get_album_media", { albumId });
      setAlbumMedia(result);
    } catch {
      setAlbumMedia([]);
    }
  }, []);

  const handleCreateAlbum = async () => {
    if (!newName.trim()) return;
    try {
      await invoke("create_album", {
        name: newName.trim(),
        description: newDesc.trim() || null,
      });
      setNewName("");
      setNewDesc("");
      setShowCreate(false);
      loadAlbums();
    } catch {
      // Handle error
    }
  };

  const handleDeleteAlbum = async (albumId: string) => {
    try {
      await invoke("delete_album", { albumId });
      if (selectedAlbum?.id === albumId) {
        setSelectedAlbum(null);
        setAlbumMedia([]);
      }
      loadAlbums();
    } catch {
      // Handle error
    }
  };

  const handleSelectAlbum = (album: Album) => {
    setSelectedAlbum(album);
    loadAlbumMedia(album.id);
  };

  const handleRemoveFromAlbum = async (mediaId: string) => {
    if (!selectedAlbum) return;
    try {
      await invoke("remove_media_from_album", {
        albumId: selectedAlbum.id,
        mediaIds: [mediaId],
      });
      loadAlbumMedia(selectedAlbum.id);
      loadAlbums();
    } catch {
      // Handle error
    }
  };

  // Album detail view
  if (selectedAlbum) {
    return (
      <div className="flex-1 flex flex-col overflow-hidden">
        <div className="flex items-center gap-3 px-4 py-3 border-b border-border bg-bg-secondary/50">
          <button
            onClick={() => {
              setSelectedAlbum(null);
              setAlbumMedia([]);
            }}
            className="p-1 rounded hover:bg-bg-secondary text-text-secondary hover:text-text-primary transition-colors"
          >
            <ArrowLeft size={16} />
          </button>
          <div>
            <h2 className="text-sm font-semibold text-text-primary">
              {selectedAlbum.name}
            </h2>
            {selectedAlbum.description && (
              <p className="text-[10px] text-text-secondary">
                {selectedAlbum.description}
              </p>
            )}
          </div>
          <span className="text-xs text-text-secondary ml-auto">
            {albumMedia.length} {t("map.photos")}
          </span>
        </div>

        <div className="flex-1 overflow-y-auto p-3">
          {albumMedia.length === 0 ? (
            <div className="text-center py-8">
              <ImageOff size={32} className="mx-auto mb-2 text-text-secondary/30" />
              <p className="text-xs text-text-secondary">{t("albums.addMedia")}</p>
            </div>
          ) : (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-2">
              {albumMedia.map((file) => (
                <div
                  key={file.id}
                  className="group relative rounded-lg overflow-hidden border border-border hover:border-accent/50 transition-all"
                >
                  <div className="aspect-square bg-bg-secondary flex items-center justify-center">
                    {file.thumbnail ? (
                      <img
                        src={`data:image/jpeg;base64,${file.thumbnail}`}
                        alt={file.file_name}
                        className="w-full h-full object-cover"
                        loading="lazy"
                      />
                    ) : file.media_type === "image" ? (
                      <img
                        src={convertFileSrc(file.file_path)}
                        alt={file.file_name}
                        className="w-full h-full object-cover"
                        loading="lazy"
                      />
                    ) : (
                      <ImageOff size={20} className="text-text-secondary/20" />
                    )}
                  </div>
                  <button
                    onClick={() => handleRemoveFromAlbum(file.id)}
                    className="absolute top-1.5 right-1.5 p-1 rounded-full bg-black/50 text-white opacity-0 group-hover:opacity-100 hover:bg-danger transition-all"
                  >
                    <Trash2 size={10} />
                  </button>
                  <div className="p-1.5 bg-bg-primary">
                    <p className="text-xs text-text-primary truncate">{file.file_name}</p>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-bg-secondary/50">
        <div className="flex items-center gap-2">
          <FolderHeart size={18} className="text-accent" />
          <h2 className="text-sm font-semibold text-text-primary">
            {t("albums.title")}
          </h2>
          <span className="text-xs text-text-secondary">
            ({albums.length})
          </span>
        </div>
        <button
          onClick={() => setShowCreate(!showCreate)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover transition-colors"
        >
          <Plus size={13} />
          {t("albums.create")}
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {/* Create album form */}
        {showCreate && (
          <div className="bg-bg-secondary rounded-lg p-3 space-y-2">
            <h3 className="text-xs font-semibold text-text-primary">
              {t("albums.create")}
            </h3>
            <input
              type="text"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleCreateAlbum()}
              placeholder={t("albums.name")}
              className="w-full h-8 px-2.5 rounded-md text-xs bg-bg-primary border border-border text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent/50"
            />
            <input
              type="text"
              value={newDesc}
              onChange={(e) => setNewDesc(e.target.value)}
              placeholder={t("albums.description")}
              className="w-full h-8 px-2.5 rounded-md text-xs bg-bg-primary border border-border text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent/50"
            />
            <div className="flex gap-2">
              <button
                onClick={handleCreateAlbum}
                disabled={!newName.trim()}
                className="px-4 py-1.5 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover transition-colors disabled:opacity-50"
              >
                {t("albums.create")}
              </button>
              <button
                onClick={() => setShowCreate(false)}
                className="px-4 py-1.5 rounded-md text-xs font-medium bg-bg-primary text-text-secondary hover:text-text-primary border border-border transition-colors"
              >
                {t("common.cancel")}
              </button>
            </div>
          </div>
        )}

        {/* Albums grid */}
        {loading ? (
          <div className="flex justify-center py-8">
            <div className="w-6 h-6 border-2 border-accent border-t-transparent rounded-full animate-spin" />
          </div>
        ) : albums.length === 0 ? (
          <div className="text-center py-8">
            <FolderHeart size={32} className="mx-auto mb-2 text-text-secondary/30" />
            <p className="text-xs text-text-secondary">{t("albums.noAlbums")}</p>
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3">
            {albums.map((album) => (
              <div
                key={album.id}
                onClick={() => handleSelectAlbum(album)}
                className="group relative rounded-lg border border-border bg-bg-secondary hover:border-accent/50 cursor-pointer transition-all overflow-hidden"
              >
                <div className="aspect-video bg-bg-primary flex items-center justify-center">
                  <FolderHeart size={28} className="text-text-secondary/20" />
                </div>
                <div className="p-2.5">
                  <h3 className="text-xs font-semibold text-text-primary truncate">
                    {album.name}
                  </h3>
                  {album.description && (
                    <p className="text-[10px] text-text-secondary mt-0.5 truncate">
                      {album.description}
                    </p>
                  )}
                  <p className="text-[10px] text-text-secondary mt-1">
                    {album.media_count ?? 0} {t("map.photos")}
                  </p>
                </div>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    handleDeleteAlbum(album.id);
                  }}
                  className="absolute top-2 right-2 p-1 rounded-full bg-black/50 text-white opacity-0 group-hover:opacity-100 hover:bg-danger transition-all"
                >
                  <Trash2 size={12} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
