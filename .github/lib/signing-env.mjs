import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

// desktop-ci writes the --env-stdin blob to dci_env, and its preamble loads
// every KEY=VALUE line into the build's environment. Signing secrets travel
// instead as one hex line the preamble skips: a leading "#" makes the POSIX
// shell read it as a comment, and hex has no "=" for the Windows parser. The
// first process after checkout takes that line out of the file, so no build or
// dependency step sees a signing secret in its environment or on disk.
export const SIGNING_LINE_PREFIX = "#muniment-signing-env:";

export const defaultEnvironmentFile = () =>
  process.platform === "win32" ? join(tmpdir(), "dci_env") : "/tmp/dci_env";

export const encodeSigningEnvironment = (entries) =>
  `${SIGNING_LINE_PREFIX}${Buffer.from(Object.entries(entries).map(([key, value]) => `${key}=${value}\n`).join(""), "utf8").toString("hex")}`;

export const decodeSigningEnvironment = (payload) => {
  const hex = String(payload ?? "").trim().replace(SIGNING_LINE_PREFIX, "");
  if (hex === "") return {};
  if (!/^(?:[0-9a-f]{2})+$/.test(hex)) throw new Error("signing environment is not hex");
  const entries = {};
  for (const line of Buffer.from(hex, "hex").toString("utf8").split(/\r?\n/)) {
    const eq = line.indexOf("=");
    if (eq > 0) entries[line.slice(0, eq)] = line.slice(eq + 1);
  }
  return entries;
};

// Remove the signing line from the file and return its hex payload. The rest
// of the file stays for the steps that read GH_TOKEN and the build switches.
export const takeSigningLine = (file = defaultEnvironmentFile()) => {
  if (!existsSync(file)) return "";
  const lines = readFileSync(file, "utf8").split(/\r?\n/);
  const signing = lines.filter((line) => line.startsWith(SIGNING_LINE_PREFIX));
  if (signing.length === 0) return "";
  if (signing.length > 1) throw new Error("the environment file carries more than one signing line");
  writeFileSync(file, lines.filter((line) => !line.startsWith(SIGNING_LINE_PREFIX)).join("\n"), { mode: 0o600 });
  return signing[0].slice(SIGNING_LINE_PREFIX.length);
};

export const takeSigningEnvironment = (file = defaultEnvironmentFile()) =>
  decodeSigningEnvironment(takeSigningLine(file));

// Read the hex payload a shell step captured with `node .github/lib/signing-env.mjs`
// and piped to this process. Stdin is read to its end before any child starts,
// so no child that inherits the descriptor can read the payload.
export const readSigningEnvironment = (read = () => readFileSync(0, "utf8")) => {
  let payload = "";
  try {
    payload = read();
  } catch (error) {
    if (error?.code !== "EAGAIN" && error?.code !== "EOF") throw error;
  }
  return decodeSigningEnvironment(payload);
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.stdout.write(takeSigningLine(process.argv[2] ?? defaultEnvironmentFile()));
}
