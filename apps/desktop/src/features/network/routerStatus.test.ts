import { describe, expect, it } from "vitest";

import type { NetworkSettings, PortMapping } from "../../services/library";
import { routerStatus } from "./routerStatus";

const base: NetworkSettings = {
  configuredProfile: "internet",
  activeProfile: "internet",
  torrentEngine: "ready",
  downloadPort: 44563,
  sharingPort: 51413,
  peerPort: 51413,
  portForwarding: true,
  activePortForwarding: true,
  restartRequired: false,
  downloadLimitKib: null,
  uploadLimitKib: null,
  portMapping: null,
};
const mapping = (overrides: Partial<PortMapping>): NetworkSettings => ({
  ...base,
  portMapping: { state: "mapped", protocol: null, ports: [], tried: [], ...overrides },
});
const both = (internalPort: number, externalPort = internalPort): PortMapping["ports"] => [
  { internalPort, externalPort, transport: "tcp" },
  { internalPort, externalPort, transport: "udp" },
];

describe("router status", () => {
  it("names the protocol and the ports it opened", () => {
    expect(routerStatus(mapping({ protocol: "nat_pmp", ports: both(51413) }))).toBe("Router: port 51413 open via NAT-PMP (TCP and UDP).");
    expect(routerStatus(mapping({ protocol: "upnp", ports: [...both(51413), ...both(44563, 60000)] }))).toBe(
      "Router: ports 51413 and 44563 (as 60000) open via UPnP (TCP and UDP).",
    );
  });

  it("explains silence and refusals, and says what to forward by hand", () => {
    const silent: PortMapping["tried"] = [
      { protocol: "pcp", outcome: "no_answer", code: null },
      { protocol: "nat_pmp", outcome: "no_gateway", code: null },
      { protocol: "upnp", outcome: "no_answer", code: null },
    ];
    expect(routerStatus(mapping({ state: "unavailable", tried: silent }))).toBe(
      "No router answered UPnP, NAT-PMP, or PCP. Forward TCP and UDP 51413 by hand for peers to reach you.",
    );
    expect(
      routerStatus(
        mapping({
          state: "unavailable",
          tried: [
            { protocol: "pcp", outcome: "refused", code: 8 },
            { protocol: "nat_pmp", outcome: "unsupported", code: null },
            { protocol: "upnp", outcome: "no_answer", code: null },
          ],
        }),
      ),
    ).toBe("Your router did not open Zeron's ports (PCP: refused (8); NAT-PMP: not supported; UPnP: no answer). Forward TCP and UDP 51413 by hand for peers to reach you.");
  });

  it("reports probing, and nothing outside the internet profile", () => {
    expect(routerStatus(mapping({ state: "probing" }))).toBe("Asking your router to open port 51413…");
    expect(routerStatus(base)).toBe("Router port opening is off.");
    expect(routerStatus({ ...base, activeProfile: "local_network" })).toBeNull();
  });
});
