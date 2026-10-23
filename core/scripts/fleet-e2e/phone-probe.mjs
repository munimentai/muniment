// The phone of the Docker fleet test, on the relay control channel.
//
// It signs a desktop and a phone in to a local cloud API, pairs them with
// revision 2 of the pairing contract (X25519 agreement, HMAC proof, HKDF
// keys) and hands the desktop half of the pair to the test runtime through
// desktop.json. Then it acts as the phone: it opens the pair's control
// session, seals requests under muniment.relay-encryption/1, reads
// `fleet.snapshot` into snapshot.json, posts the text fleet.json names into
// its thread and follows the op until it is applied or failed, which
// done.json records.
//
// Env: API (the cloud API base URL), EMAIL and PASSWORD (a local account),
// DIR (the folder it shares with the test).
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const { API, EMAIL, PASSWORD, DIR } = process.env;
const log = (...args) => console.error('phone', new Date().toISOString().slice(11, 23), ...args);
const b64 = (bytes) => Buffer.from(bytes).toString('base64url');
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function http(route, { method = 'GET', body, token, form, cookie } = {}) {
  const headers = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (cookie) headers.Cookie = cookie;
  let payload;
  if (form) {
    headers['Content-Type'] = 'application/x-www-form-urlencoded';
    payload = new URLSearchParams(form).toString();
  } else if (body !== undefined) {
    headers['Content-Type'] = 'application/json';
    payload = JSON.stringify(body);
  }
  const response = await fetch(new URL(route, API), { method, headers, body: payload, redirect: 'manual' });
  const text = await response.text();
  let json = null;
  try { json = JSON.parse(text); } catch {}
  return { status: response.status, headers: response.headers, json, text };
}

function signed(route, body, challenge, privateKey) {
  const proof = { challenge, issued_at: new Date().toISOString(), jti: b64(crypto.randomBytes(16)) };
  const canonical = JSON.stringify(body, Object.keys(body).sort());
  const transcript = ['MUNIMENT-NATIVE-V1', 'POST', route, crypto.createHash('sha256').update(canonical).digest('hex'),
    challenge, proof.issued_at, proof.jti].join('\n');
  return { ...body, device_proof: { ...proof, signature: b64(crypto.sign(null, Buffer.from(transcript), privateKey)) } };
}

async function signIn(role) {
  const clientId = role === 'desktop' ? 'muniment-desktop' : 'muniment-mobile';
  const redirect = role === 'desktop' ? 'http://127.0.0.1:49152/callback' : 'muniment://oauth/callback';
  const keys = crypto.generateKeyPairSync('ed25519');
  const publicKey = b64(keys.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32));
  const registration = await http('/v1/auth/native/devices', { method: 'POST', body: { client_id: clientId,
    client_role: role, platform: role === 'desktop' ? 'desktop' : 'ios', installation_public_key: publicKey } });
  if (registration.status !== 201) throw new Error(`${role} registration answered ${registration.status} ${registration.text}`);
  const registered = registration.json;
  const verifier = b64(crypto.randomBytes(32));
  const authorization = await http('/v1/auth/native/authorize', { method: 'POST', body: signed('/v1/auth/native/authorize', {
    client_id: clientId, client_role: role, redirect_uri: redirect, response_type: 'code',
    code_challenge: crypto.createHash('sha256').update(verifier).digest('base64url'), code_challenge_method: 'S256',
    state: b64(crypto.randomBytes(32)), device_id: registered.device_id, registration_token: registered.registration_token,
  }, registered.device_challenge, keys.privateKey) });
  if (authorization.status !== 200) throw new Error(`${role} authorize answered ${authorization.status} ${authorization.text}`);
  const authorized = authorization.json;
  const page = new URL(authorized.authorization_url).pathname;
  const login = await http(page, { method: 'POST', form: { action: 'login', email: EMAIL, password: PASSWORD } });
  if (login.status !== 303) throw new Error(`${role} login answered ${login.status}`);
  const cookie = login.headers.getSetCookie().map((value) => value.split(';')[0]).join('; ');
  const approval = await http(page, { method: 'POST', form: { action: 'approve' }, cookie });
  if (approval.status !== 302) throw new Error(`${role} approval answered ${approval.status}`);
  const code = new URL(approval.headers.get('location')).searchParams.get('code');
  const token = await http('/v1/auth/native/token', { method: 'POST', body: signed('/v1/auth/native/token', {
    grant_type: 'authorization_code', code, redirect_uri: redirect, client_id: clientId, code_verifier: verifier,
    device_id: registered.device_id }, authorized.device_challenge, keys.privateKey) });
  if (token.status !== 200) throw new Error(`${role} token answered ${token.status} ${token.text}`);
  return { deviceId: registered.device_id, token: token.json.access_token };
}

const rawX25519 = (key) => key.export({ format: 'der', type: 'spki' }).subarray(-32);
const x25519Public = (raw) => crypto.createPublicKey({ key: Buffer.concat([
  Buffer.from('302a300506032b656e032100', 'hex'), Buffer.from(raw)]), format: 'der', type: 'spki' });

async function pair(desktop, mobile) {
  const desktopKeys = crypto.generateKeyPairSync('x25519');
  const secret = crypto.randomBytes(32);
  const desktopAgreement = b64(rawX25519(desktopKeys.publicKey));
  const challenge = await http('/v1/remote-control/pairing/challenges', { method: 'POST', token: desktop.token,
    body: { contract_version: 'muniment.remote-control-pairing/2', desktop_agreement_key: desktopAgreement } });
  if (challenge.status !== 201) throw new Error(`challenge answered ${challenge.status} ${challenge.text}`);
  const qr = challenge.json.qr;
  if (qr.desktop_agreement_key !== desktopAgreement || qr.desktop_device_id !== desktop.deviceId) throw new Error('qr mismatch');
  const mobileKeys = crypto.generateKeyPairSync('x25519');
  const mobileAgreement = b64(rawX25519(mobileKeys.publicKey));
  const transcript = ['muniment.relay-encryption/1 mobile-agreement', qr.desktop_device_id, mobile.deviceId,
    qr.challenge, qr.desktop_agreement_key, mobileAgreement].join('\n');
  const proof = b64(crypto.createHmac('sha256', secret).update(transcript).digest());
  const paired = await http('/v1/remote-control/pairing', { method: 'POST', token: mobile.token, body: {
    contract_version: 'muniment.remote-control-pairing/2', desktop_device_id: qr.desktop_device_id,
    desktop_public_key: qr.desktop_public_key, desktop_agreement_key: qr.desktop_agreement_key, challenge: qr.challenge,
    mobile_agreement_key: mobileAgreement, mobile_agreement_proof: proof } });
  if (paired.status !== 201) throw new Error(`authorize answered ${paired.status} ${paired.text}`);
  const record = paired.json.pair;
  // The desktop reads the pair and checks the phone's proof before it derives keys.
  const read = (await http('/v1/remote-control/pairing', { token: desktop.token })).json.pair;
  const expected = b64(crypto.createHmac('sha256', secret).update(['muniment.relay-encryption/1 mobile-agreement',
    read.desktop_device_id, read.mobile_device_id, qr.challenge, read.desktop_agreement_key, read.mobile_agreement_key].join('\n')).digest());
  if (expected !== read.mobile_agreement_proof) throw new Error('the phone proof does not verify');
  const shared = crypto.diffieHellman({ privateKey: desktopKeys.privateKey,
    publicKey: x25519Public(Buffer.from(read.mobile_agreement_key, 'base64url')) });
  const info = (direction) => Buffer.concat([Buffer.from(`muniment.relay-encryption/1 ${direction}`), Buffer.from([0]),
    Buffer.from(record.pair_id), Buffer.from(read.desktop_agreement_key, 'base64url'), Buffer.from(read.mobile_agreement_key, 'base64url')]);
  const key = (direction) => Buffer.from(crypto.hkdfSync('sha256', shared, secret, info(direction), 32));
  return { pair: record, desktopToPhone: key('desktop-to-phone'), phoneToDesktop: key('phone-to-desktop') };
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value && typeof value === 'object') {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

function seal(key, header, plaintext) {
  const nonce = crypto.randomBytes(12);
  const cipher = crypto.createCipheriv('aes-256-gcm', key, nonce, { authTagLength: 16 });
  cipher.setAAD(Buffer.from(canonical(header)));
  const sealed = Buffer.concat([cipher.update(plaintext, 'utf8'), cipher.final(), cipher.getAuthTag()]);
  return { contract_version: 'muniment.relay-encryption/1', header, nonce: b64(nonce), ciphertext: b64(sealed) };
}

function open(key, payload) {
  const bytes = Buffer.from(payload.ciphertext, 'base64url');
  const decipher = crypto.createDecipheriv('aes-256-gcm', key, Buffer.from(payload.nonce, 'base64url'), { authTagLength: 16 });
  decipher.setAAD(Buffer.from(canonical(payload.header)));
  decipher.setAuthTag(bytes.subarray(bytes.length - 16));
  return JSON.parse(Buffer.concat([decipher.update(bytes.subarray(0, bytes.length - 16)), decipher.final()]).toString('utf8'));
}

class Phone {
  constructor(token, keys) {
    this.token = token; this.keys = keys; this.counter = 0n; this.pending = new Map(); this.waiters = [];
    this.lastSequence = 0n; this.lastCounter = 0n; this.pages = new Map();
  }
  connect() {
    return new Promise((resolve, reject) => {
      this.socket = new WebSocket(`${API.replace(/^http/, 'ws')}/v1/events`, { headers: { Authorization: `Bearer ${this.token}` } });
      this.socket.onerror = (error) => reject(error);
      this.socket.onmessage = (message) => {
        const frame = JSON.parse(message.data);
        if (frame.type === 'connection.ready') { this.identity = frame.data; resolve(); return; }
        this.receive(frame);
      };
    });
  }
  envelope(type, sessionId, payload, messageId = `msg_${crypto.randomUUID().replaceAll('-', '')}`) {
    return { protocol: 'muniment.session-relay/1', type, message_id: messageId, session_id: sessionId,
      connection_id: this.identity.connection_id, device_id: this.identity.device_id,
      actor: { user_id: this.identity.user_id, device_id: this.identity.device_id, role: 'mobile' },
      sequence: 0, timestamp: new Date().toISOString(), payload };
  }
  next(check, timeout = 20000) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('timed out')), timeout);
      this.waiters.push({ check, resolve: (value) => { clearTimeout(timer); resolve(value); } });
    });
  }
  receive(frame) {
    if (frame.type === 'control.heartbeat' && frame.payload?.kind === 'ping') {
      this.socket.send(JSON.stringify(this.envelope('control.heartbeat', null, { kind: 'pong', nonce: frame.payload.nonce })));
      return;
    }
    if (frame.type === 'session.event' && frame.session_id === this.session) this.control(frame);
    for (const waiter of [...this.waiters]) {
      if (waiter.check(frame)) { this.waiters.splice(this.waiters.indexOf(waiter), 1); waiter.resolve(frame); }
    }
  }
  control(frame) {
    const header = frame.payload.header;
    if (header.header_type !== 'control' || header.session_id !== frame.session_id) throw new Error('not a control header');
    const opened = open(this.keys.desktopToPhone, frame.payload);
    const sequence = BigInt(header.sequence);
    const counter = BigInt(opened.counter);
    // The phone's replay rule: both the header sequence and the counter move forward.
    if (sequence <= this.lastSequence || counter <= this.lastCounter) return;
    this.lastSequence = sequence; this.lastCounter = counter;
    const content = opened.content;
    if (content.type === 'event') return;
    const pages = this.pages.get(content.request_id) ?? [];
    pages[content.page] = content.body;
    this.pages.set(content.request_id, pages);
    if (pages.filter((page) => page !== undefined).length === content.pages) {
      this.pending.get(content.request_id)?.(JSON.parse(pages.join('')));
    }
  }
  async open() {
    for (let attempt = 0; attempt < 120; attempt += 1) {
      this.socket.send(JSON.stringify(this.envelope('control.open', null, {})));
      const reply = await this.next((frame) => frame.type === 'control.opened' || (frame.type === 'protocol.error' && frame.session_id === null));
      if (reply.type === 'control.opened' && reply.payload.live) return reply.payload;
      await sleep(1000);
    }
    throw new Error('the control session never went live');
  }
  async subscribe(opened) {
    this.session = opened.session_id;
    this.socket.send(JSON.stringify(this.envelope('session.subscribe', this.session,
      { after_sequence: opened.latest_sequence, limit: 100 })));
    const reply = await this.next((frame) => frame.session_id === this.session &&
      (frame.type === 'session.subscribed' || frame.type === 'protocol.error'));
    if (reply.type !== 'session.subscribed') throw new Error(`subscribe: ${JSON.stringify(reply.payload)}`);
  }
  async request(operation, body = {}) {
    this.counter += 1n;
    const requestId = `req_${crypto.randomUUID().replaceAll('-', '')}`;
    const created = new Date();
    const header = { contract_version: 'muniment.relay-encryption/1', header_type: 'request', request_id: requestId,
      session_id: this.session, created_at: created.toISOString(), expires_at: new Date(created.getTime() + 60000).toISOString() };
    const plaintext = JSON.stringify({ counter: this.counter.toString(), content: { type: 'request', request_id: requestId, operation, body } });
    const answered = new Promise((resolve) => this.pending.set(requestId, resolve));
    const receipt = this.next((frame) => (frame.type === 'control.receipt' || frame.type === 'control.failure') &&
      frame.payload?.request_id === requestId);
    this.socket.send(JSON.stringify(this.envelope('control.request', this.session, seal(this.keys.phoneToDesktop, header, plaintext), requestId)));
    const stage = await receipt;
    if (stage.type === 'control.failure') throw new Error(`${operation}: relay ${JSON.stringify(stage.payload)}`);
    return Promise.race([answered, sleep(30000).then(() => ({ ok: false, error: { code: 'timeout' } }))]);
  }
}

async function main() {
  if (!API || !EMAIL || !PASSWORD || !DIR) throw new Error('set API, EMAIL, PASSWORD and DIR');
  const fleet = JSON.parse(fs.readFileSync(path.join(DIR, 'fleet.json'), 'utf8'));
  const desktop = await signIn('desktop');
  const mobile = await signIn('mobile');
  const paired = await pair(desktop, mobile);
  log('paired', paired.pair.pair_id);
  fs.writeFileSync(path.join(DIR, 'desktop.json'), JSON.stringify({
    access_token: desktop.token, pair_id: paired.pair.pair_id, mobile_device_id: paired.pair.mobile_device_id,
    created_at: paired.pair.created_at, desktop_to_phone: b64(paired.desktopToPhone), phone_to_desktop: b64(paired.phoneToDesktop),
  }));
  const phone = new Phone(mobile.token, paired);
  await phone.connect();
  await phone.subscribe(await phone.open());
  log('control session open');

  const snapshot = await phone.request('fleet.snapshot', {});
  if (!snapshot.ok) throw new Error(`fleet.snapshot: ${JSON.stringify(snapshot)}`);
  fs.writeFileSync(path.join(DIR, 'snapshot.json'), JSON.stringify(snapshot.result));
  log(`snapshot: ${snapshot.result.members.length} members, ${snapshot.result.agents.length} agents, ${snapshot.result.threads.length} threads`);

  const post = await phone.request('thread.post', { thread_id: fleet.thread_id, text: fleet.text });
  if (!post.ok || !post.result.op_id) throw new Error(`thread.post: ${JSON.stringify(post)}`);
  let op = null;
  for (let attempt = 0; attempt < 120; attempt += 1) {
    const status = await phone.request('fleet.op_status', { op_id: post.result.op_id });
    op = status.result?.op;
    if (op?.state === 'applied' || op?.state === 'failed') break;
    await sleep(1000);
  }
  log(`post ${post.result.op_id}: ${op?.state}`);
  fs.writeFileSync(path.join(DIR, 'done.json'), JSON.stringify({
    op_id: post.result.op_id, op_state: op?.state ?? null, owner_runtime: op?.owner_runtime ?? null,
  }));
  await http('/v1/remote-control/pairing', { method: 'DELETE', token: mobile.token,
    body: { contract_version: 'muniment.remote-control-pairing/2', pair_id: paired.pair.pair_id } });
  phone.socket.close();
}

main().catch((error) => { console.error(error); process.exit(1); });
