import type { MappingProtocol, NetworkSettings, PortMapping } from "../../services/library";

const PROTOCOL_NAMES: Record<MappingProtocol, string> = { pcp: "PCP", nat_pmp: "NAT-PMP", upnp: "UPnP" };

const OUTCOMES: Record<PortMapping["tried"][number]["outcome"], string> = {
  mapped: "opened",
  no_answer: "no answer",
  refused: "refused",
  unsupported: "not supported",
  no_gateway: "no router found",
  failed: "failed",
};

function list(items: string[]): string {
  return items.length <= 1 ? (items[0] ?? "") : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/** One sentence on what the router did with Zeron's ports, or null when forwarding does not apply. */
export function routerStatus(settings: NetworkSettings): string | null {
  const mapping = settings.portMapping;
  const port = settings.sharingPort ?? settings.peerPort;
  const byHand = port ? ` Forward TCP and UDP ${port} by hand for peers to reach you.` : "";
  if (!mapping) return settings.activeProfile === "internet" ? "Router port opening is off." : null;
  switch (mapping.state) {
    case "probing":
      return port ? `Asking your router to open port ${port}…` : "Asking your router to open Zeron's ports…";
    case "stopped":
      return null;
    case "mapped": {
      const internal = [...new Set(mapping.ports.map((mapped) => mapped.internalPort))];
      const named = internal.map((internalPort) => {
        const external = mapping.ports.find((mapped) => mapped.internalPort === internalPort)?.externalPort;
        return external && external !== internalPort ? `${internalPort} (as ${external})` : String(internalPort);
      });
      const via = mapping.protocol ? ` via ${PROTOCOL_NAMES[mapping.protocol]}` : "";
      return `Router: ${internal.length === 1 ? "port" : "ports"} ${list(named)} open${via} (TCP and UDP).`;
    }
    case "unavailable": {
      const silent = mapping.tried.every((attempt) => attempt.outcome === "no_answer" || attempt.outcome === "no_gateway");
      if (silent) return `No router answered UPnP, NAT-PMP, or PCP.${byHand}`;
      const answers = mapping.tried.map(
        (attempt) => `${PROTOCOL_NAMES[attempt.protocol]}: ${OUTCOMES[attempt.outcome]}${attempt.code !== null ? ` (${attempt.code})` : ""}`,
      );
      return `Your router did not open Zeron's ports (${answers.join("; ")}).${byHand}`;
    }
  }
}
