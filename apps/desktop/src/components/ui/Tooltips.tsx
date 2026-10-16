import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

const TARGETS = "button, [role='tab'], a[href], select, input[type='range'], [data-tooltip]";
const DELAY_MS = 350;

/**
 * The tooltip text of a control: `data-tooltip`, else its `title`, else an
 * `aria-label` that says more than its visible text (icon-only buttons).
 */
export function tooltipText(element: HTMLElement): string | null {
  const explicit = element.dataset.tooltip ?? element.dataset.zrTitle ?? element.getAttribute("title");
  if (explicit?.trim()) return explicit.trim();
  const label = element.getAttribute("aria-label")?.trim();
  const visible = element.textContent?.replace(/\s+/g, " ").trim() ?? "";
  return label && label !== visible ? label : null;
}

/**
 * One tooltip for the whole app. Hovering or focusing any button, tab, link,
 * or select that has something to explain shows a themed tooltip; the
 * browser's own `title` tooltip is held back while it shows.
 */
export function Tooltips() {
  const [tip, setTip] = useState<{ text: string; left: number; top: number; below: boolean } | null>(null);
  const current = useRef<HTMLElement | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const restore = (element: HTMLElement | null) => {
      if (element?.dataset.zrTitle !== undefined) {
        element.setAttribute("title", element.dataset.zrTitle);
        delete element.dataset.zrTitle;
      }
    };
    const hide = () => {
      if (timer.current) clearTimeout(timer.current);
      restore(current.current);
      current.current = null;
      setTip(null);
    };
    const show = (element: HTMLElement, delay: number) => {
      if (current.current === element) return;
      hide();
      const text = tooltipText(element);
      if (!text) return;
      current.current = element;
      // Hold back the native tooltip so only one shows.
      const title = element.getAttribute("title");
      if (title !== null) {
        element.dataset.zrTitle = title;
        element.removeAttribute("title");
      }
      timer.current = setTimeout(() => {
        if (current.current !== element || !element.isConnected) return;
        const box = element.getBoundingClientRect();
        const below = box.top < 40;
        setTip({ text, left: box.left + box.width / 2, top: below ? box.bottom + 6 : box.top - 6, below });
      }, delay);
    };
    const over = (event: Event) => {
      const element = (event.target as Element | null)?.closest?.<HTMLElement>(TARGETS);
      if (element) show(element, DELAY_MS);
      else if (current.current) hide();
    };
    const focus = (event: FocusEvent) => {
      const element = (event.target as Element | null)?.closest?.<HTMLElement>(TARGETS);
      if (element?.matches(":focus-visible")) show(element, 0);
    };
    const key = (event: KeyboardEvent) => event.key === "Escape" && hide();
    document.addEventListener("pointerover", over);
    document.addEventListener("focusin", focus);
    document.addEventListener("focusout", hide);
    document.addEventListener("pointerdown", hide);
    document.addEventListener("keydown", key);
    window.addEventListener("scroll", hide, true);
    return () => {
      hide();
      document.removeEventListener("pointerover", over);
      document.removeEventListener("focusin", focus);
      document.removeEventListener("focusout", hide);
      document.removeEventListener("pointerdown", hide);
      document.removeEventListener("keydown", key);
      window.removeEventListener("scroll", hide, true);
    };
  }, []);

  if (!tip) return null;
  // In full screen only the full-screen element is visible, so show the tip inside it.
  const host = document.fullscreenElement ?? document.body;
  const left = Math.min(Math.max(tip.left, 80), window.innerWidth - 80);
  return createPortal(
    <div
      role="tooltip"
      className="pointer-events-none fixed z-[100] max-w-64 -translate-x-1/2 rounded-md bg-[#1c1917] px-2 py-1 text-center text-xs font-medium text-[#f5f5f4] shadow-lg"
      style={{ left, top: tip.top, transform: `translate(-50%, ${tip.below ? "0" : "-100%"})` }}
    >
      {tip.text}
    </div>,
    host,
  );
}
