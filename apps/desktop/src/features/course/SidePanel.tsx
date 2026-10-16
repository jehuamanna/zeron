import { Captions, NotebookPen, PenTool, X } from "lucide-react";
import { useRef, type ReactNode } from "react";

import { SIDE_PANEL_MAX, SIDE_PANEL_MIN, useWorkspace, type SideTab } from "../../stores/workspace";

const TABS: Array<{ value: SideTab; label: string; icon: typeof NotebookPen }> = [
  { value: "notes", label: "Notes", icon: NotebookPen },
  { value: "sketch", label: "Sketch", icon: PenTool },
  { value: "transcript", label: "Transcript", icon: Captions },
];

/** The tabbed panel beside the lesson: notes, sketch, and the video's transcript. */
export function SidePanel({ tabs, active, onClose, resizable, children }: {
  /** Tabs offered for what is open; the transcript only applies to a video. */
  tabs: SideTab[];
  active: SideTab;
  onClose: () => void;
  resizable: boolean;
  children: ReactNode;
}) {
  const { showSide, sideWidth, setSideWidth } = useWorkspace();
  const drag = useRef<{ x: number; width: number } | null>(null);

  return (
    <div className="relative flex h-full min-h-0 flex-col bg-surface">
      {resizable ? (
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize side panel"
          aria-valuemin={SIDE_PANEL_MIN}
          aria-valuemax={SIDE_PANEL_MAX}
          aria-valuenow={sideWidth}
          tabIndex={0}
          title="Drag to resize"
          className="absolute inset-y-0 -left-1.5 z-20 w-3 cursor-col-resize touch-none outline-none after:absolute after:inset-y-0 after:left-1.5 after:w-px after:bg-transparent after:transition hover:after:bg-accent focus-visible:after:bg-accent"
          onPointerDown={(event) => {
            event.currentTarget.setPointerCapture(event.pointerId);
            drag.current = { x: event.clientX, width: sideWidth };
          }}
          onPointerMove={(event) => {
            if (drag.current) setSideWidth(drag.current.width + (drag.current.x - event.clientX));
          }}
          onPointerUp={() => {
            drag.current = null;
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowLeft") setSideWidth(sideWidth + 24);
            if (event.key === "ArrowRight") setSideWidth(sideWidth - 24);
          }}
        />
      ) : null}
      <div className="flex h-11 shrink-0 items-center gap-1 border-b border-line px-2">
        <div role="tablist" aria-label="Lesson tools" className="flex min-w-0 flex-1 items-center gap-0.5">
          {TABS.filter((tab) => tabs.includes(tab.value)).map(({ value, label, icon: Icon }) => (
            <button
              key={value}
              type="button"
              role="tab"
              id={`side-tab-${value}`}
              aria-selected={active === value}
              aria-controls="side-panel-body"
              onClick={() => showSide(true, value)}
              className={`relative flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-sm font-medium transition ${active === value ? "text-fg" : "text-muted hover:bg-white/5 hover:text-fg"}`}
            >
              <Icon size={15} /> {label}
              {active === value ? <span aria-hidden className="absolute inset-x-2 -bottom-[7px] h-0.5 rounded-full bg-accent" /> : null}
            </button>
          ))}
        </div>
        <button type="button" aria-label="Close panel" title="Close panel (])" onClick={onClose} className="grid size-8 place-items-center rounded-md text-muted hover:bg-white/10 hover:text-fg">
          <X size={16} />
        </button>
      </div>
      <div id="side-panel-body" role="tabpanel" aria-labelledby={`side-tab-${active}`} className="min-h-0 flex-1">
        {children}
      </div>
    </div>
  );
}
