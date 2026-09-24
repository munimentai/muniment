import { describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { generateKeyPairSync, randomBytes, scryptSync } from "node:crypto";
import {
  decodePublicKey,
  decodeSecretKey,
  scryptParameters,
  signUpdaterBytes,
  verifyUpdaterSignature,
} from "./updater-signature.mjs";

// Build a minisign key pair in the test, so no secret key is committed. The
// small scrypt limits keep the test fast and exercise the same decryption.
const keyPair = (password) => {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const seed = privateKey.export({ format: "der", type: "pkcs8" }).subarray(-32);
  const pk = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const keyId = randomBytes(8);
  const salt = randomBytes(32);
  const opslimit = 32768n;
  const memlimit = 16n * 1024n * 1024n;
  const keynum = Buffer.concat([keyId, seed, pk, Buffer.alloc(32)]);
  const { N, r, p } = scryptParameters(opslimit, memlimit);
  const stream = scryptSync(password, salt, keynum.length, { N, r, p });
  for (let index = 0; index < keynum.length; index += 1) keynum[index] ^= stream[index];
  const limits = Buffer.alloc(16);
  limits.writeBigUInt64LE(opslimit, 0);
  limits.writeBigUInt64LE(memlimit, 8);
  const secret = Buffer.concat([Buffer.from("EdScB2", "latin1"), salt, limits, keynum]);
  const secretText = `untrusted comment: rsign encrypted secret key\n${secret.toString("base64")}\n`;
  const publicText = `untrusted comment: minisign public key: ${keyId.toString("hex").toUpperCase()}\n${Buffer.concat([Buffer.from("Ed", "latin1"), keyId, pk]).toString("base64")}\n`;
  return { secret: Buffer.from(secretText).toString("base64"), public: Buffer.from(publicText).toString("base64") };
};

describe("updater signatures", () => {
  const data = Buffer.from("bundle bytes");
  const keys = keyPair("fixture pass");

  it("matches libsodium's scrypt limits for minisign keys", () => {
    expect(scryptParameters(1048576n, 33554432n)).toEqual({ N: 32768, r: 8, p: 1 });
    expect(scryptParameters(33554432n, 1073741824n)).toEqual({ N: 1048576, r: 8, p: 1 });
  });

  it("signs with an encrypted key and verifies against its public key", () => {
    const key = decodeSecretKey(keys.secret, "fixture pass");
    expect(key.keyId.equals(decodePublicKey(keys.public).keyId)).toBe(true);
    const signature = signUpdaterBytes(data, key, { fileName: "muniment.app.tar.gz", version: "1.2.3", timestamp: 1700000000 });
    const text = Buffer.from(signature, "base64").toString("utf8");
    expect(text.split("\n")[0]).toBe("untrusted comment: signature from tauri secret key");
    expect(text.split("\n")[2]).toBe("trusted comment: timestamp:1700000000\tfile:muniment.app.tar.gz\tversion:1.2.3");
    expect(verifyUpdaterSignature(data, signature, keys.public)).toBe("timestamp:1700000000\tfile:muniment.app.tar.gz\tversion:1.2.3");
  });

  it("rejects a wrong password", () => {
    expect(() => decodeSecretKey(keys.secret, "wrong")).toThrow("password is wrong");
  });

  it("rejects changed bytes, another key and an edited trusted comment", () => {
    const key = decodeSecretKey(keys.secret, "fixture pass");
    const signature = signUpdaterBytes(data, key, { fileName: "a", version: "1.0.0", timestamp: 1 });
    expect(() => verifyUpdaterSignature(Buffer.from("other"), signature, keys.public)).toThrow("does not match the file");
    expect(() => verifyUpdaterSignature(data, signature, keyPair("x").public)).toThrow("different key");
    const edited = Buffer.from(Buffer.from(signature, "base64").toString("utf8").replace("version:1.0.0", "version:9.9.9")).toString("base64");
    expect(() => verifyUpdaterSignature(data, edited, keys.public)).toThrow("trusted comment");
  });

  it("verifies the committed updater key format", () => {
    const committed = fs.readFileSync("src-tauri/updater.pub", "utf8");
    expect(decodePublicKey(committed).keyId.toString("hex").toUpperCase()).toBe("D2817FAAE2167D33");
  });

  // Tauri's own signer must produce boxes this verifier accepts, because the
  // updater and promotion both read them.
  it.skipIf(!fs.existsSync("node_modules/@tauri-apps/cli/tauri.js"))("accepts a signature from the Tauri signer", () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "muniment-updater-signature-"));
    try {
      const keyFile = path.join(directory, "key");
      const bundle = path.join(directory, "muniment.AppImage");
      fs.writeFileSync(bundle, data);
      const cli = "node_modules/@tauri-apps/cli/tauri.js";
      const generated = spawnSync(process.execPath, [cli, "signer", "generate", "--ci", "-p", "fixture pass", "-w", keyFile], { encoding: "utf8" });
      expect(generated.status, generated.stderr).toBe(0);
      const signed = spawnSync(process.execPath, [cli, "signer", "sign", "--app-version", "1.2.3", bundle], {
        encoding: "utf8",
        env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: fs.readFileSync(keyFile, "utf8"), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "fixture pass" },
      });
      expect(signed.status, signed.stderr).toBe(0);
      const publicKey = fs.readFileSync(`${keyFile}.pub`, "utf8");
      expect(verifyUpdaterSignature(data, fs.readFileSync(`${bundle}.sig`, "utf8"), publicKey)).toMatch(/\tversion:1\.2\.3$/);
      const ours = signUpdaterBytes(data, decodeSecretKey(fs.readFileSync(keyFile, "utf8"), "fixture pass"), { fileName: "muniment.AppImage", version: "1.2.3" });
      expect(verifyUpdaterSignature(data, ours, publicKey)).toMatch(/\tfile:muniment\.AppImage\t/);
    } finally { fs.rmSync(directory, { recursive: true, force: true }); }
  });
});

