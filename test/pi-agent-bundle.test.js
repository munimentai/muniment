import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { randomUUID } from "node:crypto";
import { afterEach, describe, expect, it } from "vitest";
import { PI_DEFAULT_TOOLS, PI_PACKAGES, renderPiSettings } from "../.github/build-pi-agent.mjs";

const temporaryPaths = [];
afterEach(() => {
  for (const path of temporaryPaths.splice(0)) rmSync(path, { force: true });
});

describe("the bundled Pi agent", () => {
  it("renders the factory package and tool settings", () => {
    const output = join(tmpdir(), `muniment-pi-settings-${randomUUID()}.json`);
    temporaryPaths.push(output);
    writeFileSync(output, "stale");

    renderPiSettings(output);

    const settings = JSON.parse(readFileSync(output, "utf8"));
    expect(settings).toEqual({ packages: PI_PACKAGES, defaultTools: PI_DEFAULT_TOOLS });
    expect(settings).toEqual(JSON.parse(readFileSync("src-tauri/pi-agent/settings.json", "utf8")));
  });

  it("ships an empty project MCP template", () => {
    const template = JSON.parse(readFileSync(".pi/mcp.json", "utf8"));
    expect(template).toEqual({ mcpServers: {} });
    expect(template).toEqual(JSON.parse(readFileSync("src-tauri/pi-agent/.pi/mcp.json", "utf8")));
  });
});
