import { asCommandError, type CommandError, type DraftDetail } from "../../services/publisher";

export interface DraftWriter {
  readonly revision: number;
  /** Resolves to null when the operation made no change, such as a cancelled file picker. */
  submit<T extends DraftDetail | null>(operation: (expectedRevision: number) => Promise<T>): Promise<T>;
}

interface DraftWriterCallbacks {
  onSaved: (saved: DraftDetail) => void;
  onConflict: (currentRevision: number | undefined) => void;
}

/**
 * Serializes every write for one open draft so metadata autosave and
 * structure edits always send the latest owned revision. After a conflict,
 * all further writes are refused until the editor reloads.
 */
export function createDraftWriter(initialRevision: number, callbacks: DraftWriterCallbacks): DraftWriter {
  let revision = initialRevision;
  let conflicted = false;
  let tail: Promise<unknown> = Promise.resolve();
  return {
    get revision() {
      return revision;
    },
    submit(operation) {
      const run = tail.then(async () => {
        if (conflicted) {
          throw { code: "revision_conflict", message: "Reload the draft before editing." } satisfies CommandError;
        }
        try {
          const saved = await operation(revision);
          if (saved) {
            revision = saved.revision;
            callbacks.onSaved(saved);
          }
          return saved;
        } catch (error) {
          const command = asCommandError(error);
          if (command?.code === "revision_conflict") {
            conflicted = true;
            callbacks.onConflict(command.currentRevision);
          }
          throw error;
        }
      });
      tail = run.catch(() => undefined);
      return run;
    },
  };
}
