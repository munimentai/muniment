import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

function SignIn({ error, busy, onSubmit }) {
  const [orgId, setOrgId] = useState("");
  return <main className="signin">
    <div className="mark" aria-hidden="true" />
    <p className="eyebrow">muniment</p>
    <h1>Your organization’s governed workspace.</h1>
    <p className="lede">Sign in through your organization’s identity provider. Your session is kept in this device’s secure credential store.</p>
    <form onSubmit={(event) => { event.preventDefault(); onSubmit(orgId); }}>
      <label htmlFor="org">Organization ID</label>
      <input id="org" value={orgId} onChange={(e) => setOrgId(e.target.value)} placeholder="00000000-0000-0000-0000-000000000000" autoFocus autoComplete="organization" />
      {error && <p className="error" role="alert">{error}</p>}
      <button disabled={busy}>{busy ? "Opening sign in…" : "Continue to sign in"}</button>
    </form>
    <p className="fine">Authentication opens in a separate secure window.</p>
  </main>;
}

const label = (value) => String(value || "").replaceAll("_", " ");

function Access({ snapshot }) {
  const payload = snapshot?.payload || {};
  const grouped = useMemo(() => Object.entries((payload.grants || []).reduce((all, grant) => {
    (all[grant.resource_type] ||= []).push(grant); return all;
  }, {})), [snapshot]);
  return <section className="access" aria-labelledby="access-title">
    <div className="section-head"><div><p className="eyebrow">Your access</p><h2 id="access-title">Entitlement snapshot</h2></div><span className="version">version {payload.entitlement_version ?? "—"}</span></div>
    <div className="capabilities">
      <h3>Capabilities</h3>
      <div className="chips">{(payload.capabilities || []).length ? payload.capabilities.map(item => <span key={item}>{item}</span>) : <em>None granted</em>}</div>
    </div>
    <div className="grant-list">{grouped.length ? grouped.map(([type, grants]) => <details key={type} open>
      <summary>{label(type)} <span>{grants.length}</span></summary>
      {grants.map(grant => <div className="grant" key={grant.id}>
        <span>{grant.effect} · {grant.action}</span><code>{grant.resource_id || "*"}</code>
      </div>)}
    </details>) : <p className="empty">No resource grants in this snapshot.</p>}</div>
    <p className="snapshot-meta">signed {snapshot?.algorithm || "—"} · issued {payload.issued_at ? new Date(payload.issued_at).toLocaleString() : "—"}</p>
    <p className="fine">Access is set by your admins. The server enforces every grant.</p>
  </section>;
}

function Shell({ session, onSignOut }) {
  const email = session.user?.email || "Signed-in user";
  const name = email.split("@")[0];
  return <div className="shell">
    <aside><div><p className="wordmark">muniment</p><button className="new" disabled>＋ New thread</button><nav><span>Threads</span><p>No threads yet</p></nav></div>
      <div className="profile"><strong>{name}</strong><span>{session.user?.role || "user"} · {session.org?.id?.slice(0, 8) || "organization"}</span></div>
    </aside>
    <div className="workspace"><header><span>New thread</span><kbd>⌘K</kbd></header>
      <main className="content"><div className="welcome"><h1>Ask anything.</h1><p>Your org’s routing decides which model answers.</p></div><Access snapshot={session.entitlement_snapshot} /></main>
      <footer><span>{email}</span><button className="quiet" onClick={onSignOut}>Sign out</button></footer>
    </div>
  </div>;
}

function App() {
  const [session, setSession] = useState(null), [busy, setBusy] = useState(true), [error, setError] = useState("");
  useEffect(() => {
    if (!window.__TAURI_INTERNALS__) { setBusy(false); return; }
    const unlisten = listen("auth-complete", ({ payload }) => { setBusy(false); payload.session ? setSession(payload.session) : setError(payload.error); });
    invoke("restore_session").then(setSession).catch(setError).finally(() => setBusy(false));
    return () => { unlisten.then(fn => fn()); };
  }, []);
  const begin = async (orgId) => { setError(""); setBusy(true); try { await invoke("begin_oidc", { orgId }); } catch (e) { setError(String(e)); setBusy(false); } };
  const signOut = async () => { await invoke("sign_out"); setSession(null); };
  if (busy && !session) return <div className="loading mono">Checking session…</div>;
  return session ? <Shell session={session} onSignOut={signOut} /> : <SignIn error={error} busy={busy} onSubmit={begin} />;
}

createRoot(document.getElementById("root")).render(<App />);
