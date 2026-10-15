import "./excalidrawAssets";
import "@excalidraw/excalidraw/index.css";

import { Excalidraw } from "@excalidraw/excalidraw";
import type { BinaryFileData, BinaryFiles, ExcalidrawInitialDataState } from "@excalidraw/excalidraw/types";

import type { DrawingFile } from "../../services/library";

export interface SceneChange {
  sceneJson: string;
  files: DrawingFile[];
}

/** The Excalidraw editor. Loaded on demand: it is most of a megabyte. */
export default function DrawingCanvas({ sceneJson, files, onChange }: {
  sceneJson: string | null;
  files: DrawingFile[];
  onChange: (change: SceneChange) => void;
}) {
  const initialData = initial(sceneJson, files);
  return (
    <div className="h-[520px] overflow-hidden rounded-2xl border border-white/10">
      <Excalidraw
        initialData={initialData}
        theme="dark"
        // Embedded web pages would load remote content into the app.
        validateEmbeddable={() => false}
        UIOptions={{ canvasActions: { loadScene: false, saveToActiveFile: false, export: false, saveAsImage: false } }}
        onChange={(elements, appState, binaryFiles) => {
          const visible = elements.filter((element) => !element.isDeleted);
          const used = new Set(visible.flatMap((element) => ("fileId" in element && element.fileId ? [element.fileId] : [])));
          onChange({
            sceneJson: JSON.stringify({ elements: visible, appState: { viewBackgroundColor: appState.viewBackgroundColor } }),
            files: Object.values(binaryFiles)
              .filter((file) => used.has(file.id))
              .map((file) => ({ id: file.id, mimeType: file.mimeType, dataUrl: file.dataURL })),
          });
        }}
      />
    </div>
  );
}

function initial(sceneJson: string | null, files: DrawingFile[]): ExcalidrawInitialDataState | null {
  if (!sceneJson) return null;
  const scene = JSON.parse(sceneJson) as ExcalidrawInitialDataState;
  const binaryFiles: BinaryFiles = {};
  for (const file of files) {
    binaryFiles[file.id] = { id: file.id, mimeType: file.mimeType, dataURL: file.dataUrl, created: 0 } as BinaryFileData;
  }
  return { elements: scene.elements ?? null, appState: scene.appState ?? null, files: binaryFiles };
}
