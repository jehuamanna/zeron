import axe from "axe-core";

/**
 * Run axe-core's WCAG 2 A and AA rules on `container`. Colour contrast needs
 * real layout and is checked in the window, not in jsdom.
 */
export async function accessibilityViolations(container: Element): Promise<string[]> {
  const results = await axe.run(container, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "best-practice"] },
    rules: { "color-contrast": { enabled: false }, region: { enabled: false } },
  });
  return results.violations.map(
    (violation) => `${violation.id}: ${violation.help} — ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`,
  );
}
