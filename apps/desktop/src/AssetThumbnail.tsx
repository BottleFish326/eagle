import { useEffect, useRef, useState } from "react";

import type { DesktopApi } from "./desktop-api";
import { Icon } from "./Icon";
import {
  createTrackedObjectUrl,
  revokeTrackedObjectUrl,
} from "./object-url-registry";
import type { AssetRecord } from "./scanner";

type PreviewState =
  | { status: "waiting" }
  | { status: "loading" }
  | { status: "ready"; url: string }
  | { status: "placeholder"; message: string };

export function AssetThumbnail({
  api,
  asset,
}: {
  api: DesktopApi;
  asset: AssetRecord;
}) {
  const container = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [preview, setPreview] = useState<PreviewState>({ status: "waiting" });

  useEffect(() => {
    const element = container.current;
    if (element === null) return;
    if (!("IntersectionObserver" in window)) {
      setVisible(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: "320px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [asset.key]);

  useEffect(() => {
    if (!visible) return;
    let active = true;
    let objectUrl: string | undefined;
    setPreview({ status: "loading" });
    void api
      .requestThumbnail({ assetKey: asset.key, maxEdge: 640 })
      .then(async (outcome) => {
        if (!active) return;
        if (outcome.status === "placeholder") {
          setPreview({ status: "placeholder", message: outcome.message });
          return;
        }
        const bytes = await api.readThumbnail(outcome.thumbnail.cacheKey);
        if (!active) return;
        objectUrl = createTrackedObjectUrl(
          new Blob([bytes], { type: outcome.thumbnail.mime }),
        );
        setPreview({ status: "ready", url: objectUrl });
      })
      .catch((error: unknown) => {
        if (active) {
          setPreview({
            status: "placeholder",
            message: error instanceof Error ? error.message : "缩略图读取失败",
          });
        }
      });
    return () => {
      active = false;
      if (objectUrl !== undefined) revokeTrackedObjectUrl(objectUrl);
    };
  }, [api, asset.key, visible]);

  return (
    <div
      className={`asset-preview asset-preview--${preview.status}`}
      ref={container}
    >
      {preview.status === "ready" ? (
        <img alt="" draggable={false} src={preview.url} />
      ) : preview.status === "placeholder" ? (
        <span className="preview-placeholder" title={preview.message}>
          <Icon name="alert" size={22} />
          <span>无法预览</span>
        </span>
      ) : (
        <span className="preview-placeholder" aria-label="正在载入缩略图">
          <Icon name="image" size={22} />
        </span>
      )}
    </div>
  );
}
