import "./excalidrawAssets";

import { exportToSvg } from "@excalidraw/excalidraw";
import type { BinaryFileData, BinaryFiles } from "@excalidraw/excalidraw/types";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import { libraryApi } from "../../services/library";
import { useResolvedTheme } from "../../stores/shell";

/**
 * A lesson's drawing rendered as a still image. The SVG is shown through an
 * <img>, so nothing in it can run. Loaded on demand with Excalidraw.
 */
export default function SketchPreview({ courseId, lessonId, title }: { courseId: string; lessonId: string; title: string }) {
  const drawing = useQuery({ queryKey: ["drawing-preview", courseId, lessonId], queryFn: () => libraryApi.drawing(courseId, lessonId), staleTime: 0 });
  const dark = useResolvedTheme() === "dark";
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const data = drawing.data;
    if (!data) return;
    let current = true;
    const scene = JSON.parse(data.sceneJson) as { elements?: unknown[]; appState?: Record<string, unknown> };
    const files: BinaryFiles = {};
    for (const file of data.files) {
      files[file.id] = { id: file.id, mimeType: file.mimeType, dataURL: file.dataUrl, created: 0 } as BinaryFileData;
    }
    exportToSvg({
      elements: (scene.elements ?? []) as Parameters<typeof exportToSvg>[0]["elements"],
      appState: { ...scene.appState, exportBackground: true, exportWithDarkMode: dark },
      files,
      exportPadding: 16,
    })
      .then((svg: SVGSVGElement) => current && setSrc(`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg.outerHTML)}`))
      .catch(() => current && setFailed(true));
    return () => {
      current = false;
    };
  }, [drawing.data, dark]);

  if (drawing.isError || failed) return <p className="p-4 text-xs text-red-300">This sketch could not be shown.</p>;
  if (!src) return <div aria-hidden className="h-40 animate-pulse rounded-lg bg-white/5" />;
  return <img src={src} alt={`Sketch for ${title}`} className="max-h-80 w-full rounded-lg object-contain" />;
}
