import { describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import {
  decodeSigningEnvironment,
  encodeSigningEnvironment,
  readSigningEnvironment,
  SIGNING_LINE_PREFIX,
  takeSigningEnvironment,
} from "./signing-env.mjs";

const withFile = (content, test) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "muniment-signing-env-"));
  const file = path.join(directory, "dci_env");
  fs.writeFileSync(file, content);
  try { return test(file); } finally { fs.rmSync(directory, { recursive: true, force: true }); }
};

describe("signing environment transport", () => {
  const secrets = { AZURE_CLIENT_SECRET: "a=b+c/d", APPLE_CERTIFICATE: "MIIK==" };

  it("encodes a line with no = so the Windows preamble skips it", () => {
    const line = encodeSigningEnvironment(secrets);
    expect(line.startsWith(SIGNING_LINE_PREFIX)).toBe(true);
    expect(line).not.toContain("=");
    expect(decodeSigningEnvironment(line)).toEqual(secrets);
  });

  it.skipIf(process.platform === "win32")("keeps the line out of the POSIX preamble's environment", () => {
    withFile(`GH_TOKEN=fixture\n${encodeSigningEnvironment(secrets)}\n`, (file) => {
      const result = spawnSync("bash", ["-c", `set -e; set -a; . "${file}"; set +a; env`], { encoding: "utf8" });
      expect(result.status).toBe(0);
      expect(result.stdout).toContain("GH_TOKEN=fixture");
      expect(result.stdout).not.toContain("AZURE_CLIENT_SECRET");
      expect(result.stdout).not.toContain(SIGNING_LINE_PREFIX);
    });
  });

  it.skipIf(process.platform === "win32")("matches the runner's od encoding", () => {
    const result = spawnSync("bash", ["-c", "printf '%s\\n' 'AZURE_CLIENT_SECRET=a=b+c/d' 'APPLE_CERTIFICATE=MIIK==' | od -An -v -tx1 | tr -d ' \\n'"], { encoding: "utf8" });
    expect(`${SIGNING_LINE_PREFIX}${result.stdout}`).toBe(encodeSigningEnvironment(secrets));
  });

  it("takes the line out of the file and keeps the other lines", () => {
    withFile(`GH_TOKEN=fixture\n${encodeSigningEnvironment(secrets)}\nMUNIMENT_PI_CANDIDATE=1\n`, (file) => {
      expect(takeSigningEnvironment(file)).toEqual(secrets);
      const rest = fs.readFileSync(file, "utf8");
      expect(rest).toBe("GH_TOKEN=fixture\nMUNIMENT_PI_CANDIDATE=1\n");
      expect(takeSigningEnvironment(file)).toEqual({});
    });
  });

  it("answers an empty set when no file or line exists", () => {
    expect(takeSigningEnvironment(path.join(os.tmpdir(), "muniment-missing-dci-env"))).toEqual({});
    withFile("GH_TOKEN=fixture\n", (file) => expect(takeSigningEnvironment(file)).toEqual({}));
  });

  it("rejects a damaged payload and a second line", () => {
    expect(() => decodeSigningEnvironment(`${SIGNING_LINE_PREFIX}zz`)).toThrow("not hex");
    const line = encodeSigningEnvironment(secrets);
    withFile(`${line}\n${line}\n`, (file) => expect(() => takeSigningEnvironment(file)).toThrow("more than one"));
  });

  it("reads the piped payload", () => {
    const line = encodeSigningEnvironment(secrets).slice(SIGNING_LINE_PREFIX.length);
    expect(readSigningEnvironment(() => `${line}\n`)).toEqual(secrets);
    expect(readSigningEnvironment(() => "")).toEqual({});
  });
});
