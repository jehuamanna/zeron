import type { SeedingStatus } from "../../services/publisher";

export function seedingLabel(seeding: SeedingStatus | "unavailable" | undefined) {
  if (seeding === "unavailable") return "Seeding state unavailable";
  if (!seeding || !seeding.enabled) return "Not seeding";
  switch (seeding.state) {
    case "seeding":
      return `Seeding · ${count(seeding.connectedPeers, "peer")}`;
    case "checking":
      return "Checking files";
    case "failed":
      return "Seeding failed";
    default:
      return seeding.failure ? "Seeding failed" : "Starting";
  }
}

export function count(value: number, noun: string) {
  return `${value} ${noun}${value === 1 ? "" : "s"}`;
}
