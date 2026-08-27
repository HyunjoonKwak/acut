import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { MapPin, Image } from "lucide-react";
import { useAppStore } from "@/stores/appStore";
import type { GpsMediaItem } from "@/types";

interface LocationCluster {
  key: string;
  lat: number;
  lng: number;
  items: GpsMediaItem[];
  dateRange: { min: string; max: string } | null;
}

function clusterByLocation(items: GpsMediaItem[]): LocationCluster[] {
  const groups: Record<string, GpsMediaItem[]> = {};

  for (const item of items) {
    // Round to 2 decimal places (~1km grouping)
    const lat = Math.round(item.latitude * 100) / 100;
    const lng = Math.round(item.longitude * 100) / 100;
    const key = `${lat},${lng}`;
    if (!groups[key]) groups[key] = [];
    groups[key].push(item);
  }

  return Object.entries(groups)
    .map(([key, items]) => {
      const [latStr, lngStr] = key.split(",");
      const dates = items
        .map((i) => i.date_taken)
        .filter(Boolean)
        .sort() as string[];

      return {
        key,
        lat: parseFloat(latStr),
        lng: parseFloat(lngStr),
        items,
        dateRange:
          dates.length > 0
            ? { min: dates[0], max: dates[dates.length - 1] }
            : null,
      };
    })
    .sort((a, b) => b.items.length - a.items.length);
}

function formatCoord(lat: number, lng: number): string {
  const latDir = lat >= 0 ? "N" : "S";
  const lngDir = lng >= 0 ? "E" : "W";
  return `${Math.abs(lat).toFixed(2)}${latDir}, ${Math.abs(lng).toFixed(2)}${lngDir}`;
}

export function MapView() {
  const { t } = useTranslation();
  const [media, setMedia] = useState<GpsMediaItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedCluster, setSelectedCluster] = useState<string | null>(null);

  const loadGpsMedia = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<GpsMediaItem[]>("get_gps_media");
      setMedia(result);
    } catch {
      // Command not available yet
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    loadGpsMedia();
  }, [loadGpsMedia]);

  const clusters = useMemo(() => clusterByLocation(media), [media]);

  const handleClusterClick = (cluster: LocationCluster) => {
    setSelectedCluster(
      selectedCluster === cluster.key ? null : cluster.key
    );
  };

  const handleFilterToLocation = (cluster: LocationCluster) => {
    // Switch to gallery view filtered to this location's media IDs
    useAppStore.getState().setCurrentView("gallery");
    useAppStore.getState().setSearchQuery(
      `gps:${cluster.lat},${cluster.lng}`
    );
  };

  if (loading) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="w-6 h-6 border-2 border-accent border-t-transparent rounded-full animate-spin" />
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Header */}
      <div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-bg-secondary/50">
        <MapPin size={18} className="text-accent" />
        <h2 className="text-sm font-semibold text-text-primary">
          {t("map.title")}
        </h2>
        <span className="text-xs text-text-secondary">
          ({media.length} {t("map.photos")})
        </span>
      </div>

      {media.length === 0 ? (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center">
            <MapPin size={48} className="mx-auto mb-3 text-text-secondary/30" />
            <p className="text-sm text-text-secondary">{t("map.noGps")}</p>
          </div>
        </div>
      ) : (
        <div className="flex-1 flex overflow-hidden">
          {/* Simple map visualization */}
          <div className="flex-1 relative bg-bg-secondary overflow-hidden">
            {/* World rectangle background */}
            <div className="absolute inset-4 border border-border rounded-lg bg-bg-primary/50 overflow-hidden">
              {/* Grid lines */}
              <div className="absolute inset-0 opacity-10">
                {[...Array(6)].map((_, i) => (
                  <div
                    key={`h-${i}`}
                    className="absolute w-full border-t border-text-secondary"
                    style={{ top: `${((i + 1) * 100) / 7}%` }}
                  />
                ))}
                {[...Array(11)].map((_, i) => (
                  <div
                    key={`v-${i}`}
                    className="absolute h-full border-l border-text-secondary"
                    style={{ left: `${((i + 1) * 100) / 12}%` }}
                  />
                ))}
              </div>

              {/* Location dots */}
              {clusters.map((cluster) => {
                // Map lat/lng to x/y: lng -180..180 -> 0..100%, lat 90..-90 -> 0..100%
                const x = ((cluster.lng + 180) / 360) * 100;
                const y = ((90 - cluster.lat) / 180) * 100;
                const size = Math.min(24, Math.max(8, Math.sqrt(cluster.items.length) * 4));
                const isSelected = selectedCluster === cluster.key;

                return (
                  <button
                    key={cluster.key}
                    onClick={() => handleClusterClick(cluster)}
                    className={`absolute rounded-full flex items-center justify-center transition-all transform -translate-x-1/2 -translate-y-1/2 ${
                      isSelected
                        ? "bg-accent text-white ring-2 ring-accent/30 z-10 scale-125"
                        : "bg-accent/70 text-white hover:bg-accent hover:scale-110 z-0"
                    }`}
                    style={{
                      left: `${x}%`,
                      top: `${y}%`,
                      width: `${size}px`,
                      height: `${size}px`,
                    }}
                    title={`${formatCoord(cluster.lat, cluster.lng)} - ${cluster.items.length} ${t("map.photos")}`}
                  >
                    {cluster.items.length > 1 && size >= 16 && (
                      <span className="text-[7px] font-bold">
                        {cluster.items.length}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          </div>

          {/* Cluster list sidebar */}
          <div className="w-72 border-l border-border bg-bg-primary overflow-y-auto">
            <div className="p-3 border-b border-border">
              <h3 className="text-xs font-semibold text-text-secondary uppercase tracking-wider">
                {t("map.cluster")} ({clusters.length})
              </h3>
            </div>
            <div className="divide-y divide-border/50">
              {clusters.map((cluster) => {
                const isSelected = selectedCluster === cluster.key;
                return (
                  <div key={cluster.key}>
                    <button
                      onClick={() => handleClusterClick(cluster)}
                      className={`w-full text-left p-3 transition-colors ${
                        isSelected
                          ? "bg-accent/10"
                          : "hover:bg-bg-secondary"
                      }`}
                    >
                      <div className="flex items-center gap-2">
                        <MapPin
                          size={14}
                          className={
                            isSelected ? "text-accent" : "text-text-secondary"
                          }
                        />
                        <span className="text-xs font-medium text-text-primary">
                          {formatCoord(cluster.lat, cluster.lng)}
                        </span>
                        <span className="text-[10px] text-text-secondary ml-auto">
                          {cluster.items.length} {t("map.photos")}
                        </span>
                      </div>
                      {cluster.dateRange && (
                        <p className="text-[10px] text-text-secondary mt-1 ml-5">
                          {cluster.dateRange.min.slice(0, 10)}
                          {cluster.dateRange.min.slice(0, 10) !==
                          cluster.dateRange.max.slice(0, 10)
                            ? ` ~ ${cluster.dateRange.max.slice(0, 10)}`
                            : ""}
                        </p>
                      )}
                    </button>

                    {/* Expanded thumbnails */}
                    {isSelected && (
                      <div className="px-3 pb-3 space-y-2">
                        <div className="flex flex-wrap gap-1.5">
                          {cluster.items.slice(0, 12).map((item) => (
                            <div
                              key={item.media_id}
                              className="w-14 h-14 rounded overflow-hidden bg-bg-secondary"
                            >
                              {item.thumbnail ? (
                                <img
                                  src={`data:image/jpeg;base64,${item.thumbnail}`}
                                  alt={item.file_name}
                                  className="w-full h-full object-cover"
                                />
                              ) : (
                                <div className="w-full h-full flex items-center justify-center">
                                  <Image
                                    size={12}
                                    className="text-text-secondary/30"
                                  />
                                </div>
                              )}
                            </div>
                          ))}
                          {cluster.items.length > 12 && (
                            <div className="w-14 h-14 rounded bg-bg-secondary flex items-center justify-center text-[10px] text-text-secondary">
                              +{cluster.items.length - 12}
                            </div>
                          )}
                        </div>
                        <button
                          onClick={() => handleFilterToLocation(cluster)}
                          className="w-full py-1.5 rounded text-[10px] font-medium text-accent bg-accent/10 hover:bg-accent/20 transition-colors"
                        >
                          {t("sidebar.allFiles")}
                        </button>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
