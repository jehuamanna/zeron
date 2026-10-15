import { describe, expect, it } from "vitest";

import { STALL_AFTER_MS, describePlayback, initialPlayback, playbackReducer, type Playback, type PlaybackEvent } from "./playbackState";

const run = (events: PlaybackEvent[], from: Playback = initialPlayback) => events.reduce(playbackReducer, from);

describe("playback state", () => {
  it("plays only once the position advances, not when the element claims to", () => {
    const started = run([{ type: "loadstart", at: 0 }, { type: "playing", at: 10 }, { type: "timeupdate", position: 0, paused: false, at: 20 }]);
    expect(started.state).toBe("loading");
    expect(run([{ type: "timeupdate", position: 0.4, paused: false, at: 300 }], started).state).toBe("playing");
  });

  it("treats a seek jump as buffering until playback moves on from the new position", () => {
    const playing = run([{ type: "loadstart", at: 0 }, { type: "timeupdate", position: 3, paused: false, at: 1 }]);
    const seeking = run([{ type: "seeking", at: 5 }, { type: "timeupdate", position: 1500, paused: false, at: 6 }], playing);
    expect(seeking.state).toBe("buffering");
    expect(seeking.position).toBe(1500);
    expect(run([{ type: "timeupdate", position: 1500.5, paused: false, at: 7 }], seeking).state).toBe("playing");
  });

  it("reports a stall only after a wait without any progress", () => {
    const waiting = run([{ type: "loadstart", at: 0 }, { type: "timeupdate", position: 2, paused: false, at: 1 }, { type: "waiting", at: 1000 }]);
    expect(waiting.state).toBe("buffering");
    expect(run([{ type: "tick", at: 1000 + STALL_AFTER_MS - 1 }], waiting).state).toBe("buffering");
    const progressing = run([{ type: "progress", at: 6000 }, { type: "tick", at: 1000 + STALL_AFTER_MS }], waiting);
    expect(progressing.state).toBe("buffering");
    const stalled = run([{ type: "tick", at: 6000 + STALL_AFTER_MS }], progressing);
    expect(stalled.state).toBe("stalled");
    expect(describePlayback(stalled, true)).toContain("waiting for verified data from peers");
    expect(describePlayback(stalled, true, true)).toContain("Downloads are paused");
    expect(describePlayback(stalled, false, true)).toContain("Stalled: the lesson file is not being read.");
    const resumed = run([{ type: "progress", at: 20_000 }], stalled);
    expect(resumed.state).toBe("buffering");
    expect(run([{ type: "timeupdate", position: 2.5, paused: false, at: 20_100 }], resumed).state).toBe("playing");
  });

  it("ignores WebKit's stalled events while playback advances", () => {
    const playing = run([{ type: "loadstart", at: 0 }, { type: "timeupdate", position: 1, paused: false, at: 1 }]);
    let state = playing;
    for (let second = 2; second < 30; second += 1) {
      state = run([{ type: "tick", at: second * 1000 }, { type: "timeupdate", position: second, paused: false, at: second * 1000 }], state);
    }
    expect(state.state).toBe("playing");
  });

  it("keeps errors until a new load and explains them", () => {
    const failed = run([{ type: "loadstart", at: 0 }, { type: "error", code: 4 }, { type: "timeupdate", position: 3, paused: false, at: 5 }, { type: "tick", at: 60_000 }]);
    expect(failed).toMatchObject({ state: "error", errorCode: 4 });
    expect(describePlayback(failed, true)).toBe("This lesson could not be loaded.");
    expect(describePlayback({ ...failed, errorCode: 3 }, true)).toContain("could not be decoded");
    expect(run([{ type: "loadstart", at: 70_000 }], failed).state).toBe("loading");
  });

  it("distinguishes paused and finished lessons", () => {
    const playing = run([{ type: "loadstart", at: 0 }, { type: "timeupdate", position: 1, paused: false, at: 1 }]);
    expect(run([{ type: "pause", at: 2 }], playing).state).toBe("paused");
    expect(run([{ type: "ended" }, { type: "pause", at: 3 }], playing).state).toBe("ended");
    expect(run([{ type: "pause", at: 2 }, { type: "tick", at: 60_000 }], playing).state).toBe("paused");
  });
});
