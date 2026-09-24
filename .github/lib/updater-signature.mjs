import { createHash, createPrivateKey, createPublicKey, scryptSync, sign, verify } from "node:crypto";

// Tauri updater signatures are minisign signature boxes, base64-encoded as a
// whole. This module signs and verifies them with Node's own crypto, so the
// step that holds the updater key runs no third-party code, and promotion
// checks each bundle against the committed public key.

const PKCS8_ED25519 = Buffer.from("302e020100300506032b657004220420", "hex");
const SPKI_ED25519 = Buffer.from("302a300506032b6570032100", "hex");
const UNTRUSTED_PREFIX = "untrusted comment: ";
const TRUSTED_PREFIX = "trusted comment: ";

const blake2b512 = (data) => createHash("blake2b512").update(data).digest();

// A key or signature file is text. Tauri stores it base64-encoded, so accept
// either the text or its base64 form.
const boxText = (value, label) => {
  const raw = String(value ?? "").trim();
  if (raw.startsWith(UNTRUSTED_PREFIX)) return raw;
  const decoded = Buffer.from(raw, "base64").toString("utf8");
  if (decoded.startsWith(UNTRUSTED_PREFIX)) return decoded;
  throw new Error(`${label} is not a minisign ${label}`);
};

const boxLines = (value, label) => boxText(value, label).split(/\r?\n/);

// libsodium's crypto_pwhash_scryptsalsa208sha256 turns opslimit and memlimit
// into scrypt N, r and p this way, and minisign keys use it.
export const scryptParameters = (opslimit, memlimit) => {
  const ops = Math.max(Number(opslimit), 32768);
  const r = 8;
  let logN = 1;
  let p;
  if (ops < Number(memlimit) / 32) {
    p = 1;
    const maxN = ops / (r * 4);
    while (logN < 63 && 2 ** logN <= maxN / 2) logN += 1;
  } else {
    const maxN = Number(memlimit) / (r * 128);
    while (logN < 63 && 2 ** logN <= maxN / 2) logN += 1;
    const maxrp = Math.min(Math.floor(ops / 4 / 2 ** logN), 0x3fffffff);
    p = Math.floor(maxrp / r);
  }
  return { N: 2 ** logN, r, p };
};

export const decodeSecretKey = (value, password) => {
  const bytes = Buffer.from(boxLines(value, "secret key")[1] ?? "", "base64");
  if (bytes.length !== 158) throw new Error("secret key has the wrong length");
  const algorithm = bytes.subarray(0, 2);
  const kdf = bytes.subarray(2, 4).toString("latin1");
  const checksumAlgorithm = bytes.subarray(4, 6).toString("latin1");
  if (algorithm.toString("latin1") !== "Ed" || checksumAlgorithm !== "B2") throw new Error("secret key uses an unsupported algorithm");
  const keynum = Buffer.from(bytes.subarray(54, 158));
  if (kdf === "Sc") {
    const salt = bytes.subarray(6, 38);
    const { N, r, p } = scryptParameters(bytes.readBigUInt64LE(38), bytes.readBigUInt64LE(46));
    const stream = scryptSync(Buffer.from(password ?? "", "utf8"), salt, keynum.length, { N, r, p, maxmem: 256 * N * r + 1024 * 1024 });
    for (let index = 0; index < keynum.length; index += 1) keynum[index] ^= stream[index];
  } else if (kdf !== "\0\0") {
    throw new Error("secret key uses an unsupported key derivation");
  }
  const keyId = Buffer.from(keynum.subarray(0, 8));
  const secretKey = keynum.subarray(8, 72);
  // An Ed25519 secret key is the seed followed by its public key. Node has no
  // 32-byte BLAKE2b for minisign's checksum, so derive the public key from the
  // decrypted seed instead: a wrong password yields a seed that does not match.
  const privateKey = createPrivateKey({ key: Buffer.concat([PKCS8_ED25519, secretKey.subarray(0, 32)]), format: "der", type: "pkcs8" });
  const derived = createPublicKey(privateKey).export({ format: "der", type: "spki" }).subarray(SPKI_ED25519.length);
  if (!derived.equals(secretKey.subarray(32, 64))) throw new Error("secret key password is wrong or the key is damaged");
  return { keyId, privateKey };
};

export const decodePublicKey = (value) => {
  const bytes = Buffer.from(boxLines(value, "public key")[1] ?? "", "base64");
  if (bytes.length !== 42 || bytes.subarray(0, 2).toString("latin1") !== "Ed") throw new Error("public key is not an Ed25519 minisign key");
  return {
    keyId: Buffer.from(bytes.subarray(2, 10)),
    publicKey: createPublicKey({ key: Buffer.concat([SPKI_ED25519, bytes.subarray(10, 42)]), format: "der", type: "spki" }),
  };
};

// Tauri's signer writes a prehashed signature and this trusted comment, and
// the .sig file holds the base64 of the whole box.
export const signUpdaterBytes = (data, { keyId, privateKey }, { fileName, version, timestamp = Math.floor(Date.now() / 1000) }) => {
  if (!/^[^\t\r\n]+$/.test(fileName) || !/^[^\t\r\n]+$/.test(version)) throw new Error("signature comment fields must be single-line text");
  const trustedComment = `timestamp:${timestamp}\tfile:${fileName}\tversion:${version}`;
  const signature = sign(null, blake2b512(data), privateKey);
  const globalSignature = sign(null, Buffer.concat([signature, Buffer.from(trustedComment, "utf8")]), privateKey);
  const text = `${UNTRUSTED_PREFIX}signature from tauri secret key\n`
    + `${Buffer.concat([Buffer.from("ED", "latin1"), keyId, signature]).toString("base64")}\n`
    + `${TRUSTED_PREFIX}${trustedComment}\n`
    + `${globalSignature.toString("base64")}\n`;
  return Buffer.from(text, "utf8").toString("base64");
};

// Verify a .sig against the bundle bytes and a public key, and return the
// trusted comment. Both the file signature and the global signature over the
// trusted comment must hold.
export const verifyUpdaterSignature = (data, signatureFile, publicKeyValue) => {
  const { keyId, publicKey } = decodePublicKey(publicKeyValue);
  const lines = boxLines(signatureFile, "signature");
  const bytes = Buffer.from(lines[1] ?? "", "base64");
  const trustedLine = lines[2] ?? "";
  if (bytes.length !== 74 || !trustedLine.startsWith(TRUSTED_PREFIX)) throw new Error("signature box is malformed");
  const algorithm = bytes.subarray(0, 2).toString("latin1");
  if (!bytes.subarray(2, 10).equals(keyId)) throw new Error("signature was made by a different key");
  const signature = bytes.subarray(10, 74);
  const message = algorithm === "ED" ? blake2b512(data) : algorithm === "Ed" ? data : null;
  if (!message) throw new Error("signature uses an unsupported algorithm");
  if (!verify(null, message, publicKey, signature)) throw new Error("signature does not match the file");
  const trustedComment = trustedLine.slice(TRUSTED_PREFIX.length);
  const globalSignature = Buffer.from(lines[3] ?? "", "base64");
  if (!verify(null, Buffer.concat([signature, Buffer.from(trustedComment, "utf8")]), publicKey, globalSignature)) {
    throw new Error("trusted comment signature does not match");
  }
  return trustedComment;
};
