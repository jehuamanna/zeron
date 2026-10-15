import { useCallback, useEffect, useRef, useState } from "react";

import { asCommandError, publisherApi, type DraftDetail, type DraftMetadata } from "../../services/publisher";
import type { DraftWriter } from "./draftWriter";

export interface MetadataFields {
  courseVersion: string;
  title: string;
  description: string;
  publisherName: string;
  language: string;
  category: string;
  tags: string;
  prerequisites: string;
  licenseName: string;
}

export type SaveState =
  | { kind: "saved" }
  | { kind: "dirty" }
  | { kind: "saving" }
  | { kind: "conflict" }
  | { kind: "error"; message: string; field: string | undefined };

export function fieldsFromDraft(draft: DraftDetail): MetadataFields {
  return {
    courseVersion: draft.courseVersion,
    title: draft.title,
    description: draft.description ?? "",
    publisherName: draft.publisherName,
    language: draft.language,
    category: draft.category ?? "",
    tags: draft.tags.join(", "),
    prerequisites: draft.prerequisites.join("\n"),
    licenseName: draft.licenseName ?? "",
  };
}

export function metadataFromFields(fields: MetadataFields): DraftMetadata {
  const optional = (value: string) => (value.trim() ? value : null);
  const entries = (value: string, separator: string) =>
    value
      .split(separator)
      .map((entry) => entry.trim())
      .filter(Boolean);
  return {
    courseVersion: fields.courseVersion,
    title: fields.title,
    description: optional(fields.description),
    publisherName: fields.publisherName,
    language: fields.language,
    category: optional(fields.category),
    tags: entries(fields.tags, ","),
    prerequisites: entries(fields.prerequisites, "\n"),
    licenseName: optional(fields.licenseName),
  };
}

/**
 * Debounced metadata autosave that coalesces edits made while a save is in
 * flight. Revision ownership and conflict blocking belong to the shared writer.
 */
export function useMetadataAutosave(draft: DraftDetail, writer: DraftWriter, delayMs: number) {
  const [fields, setFields] = useState(() => fieldsFromDraft(draft));
  const [state, setState] = useState<SaveState>({ kind: "saved" });
  const latest = useRef(fields);
  const inFlight = useRef(false);
  const queued = useRef(false);
  const blocked = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => () => clearTimeout(timer.current), []);

  const save = useCallback(async () => {
    if (blocked.current) return;
    if (inFlight.current) {
      queued.current = true;
      return;
    }
    inFlight.current = true;
    const snapshot = latest.current;
    setState({ kind: "saving" });
    try {
      await writer.submit((revision) =>
        publisherApi.updateDraftMetadata(draft.draftId, revision, metadataFromFields(snapshot)),
      );
      inFlight.current = false;
      if (queued.current) {
        queued.current = false;
        void save();
      } else {
        setState(latest.current === snapshot ? { kind: "saved" } : { kind: "dirty" });
      }
    } catch (error) {
      inFlight.current = false;
      queued.current = false;
      const command = asCommandError(error);
      if (command?.code === "revision_conflict") {
        blocked.current = true;
        clearTimeout(timer.current);
        setState({ kind: "conflict" });
      } else {
        setState({ kind: "error", message: command?.message ?? "The draft could not be saved.", field: command?.field });
      }
    }
  }, [draft.draftId, writer]);

  const update = (name: keyof MetadataFields, value: string) => {
    const next = { ...latest.current, [name]: value };
    latest.current = next;
    setFields(next);
    if (blocked.current) return;
    setState({ kind: "dirty" });
    clearTimeout(timer.current);
    timer.current = setTimeout(() => void save(), delayMs);
  };

  return { fields, state, update, retry: () => void save() };
}
