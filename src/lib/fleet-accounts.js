// The fleet's model accounts from core's `fleet.credentials`, and the line
// that names the machine each sign-in is signed in on. Core lists each
// account under the member that holds it, keyed `pi:<provider>` or
// `router:<id>`, with `holder`, `holder_name`, `this_machine`, `usable_here`
// and `shared`. A router account another member shares lands here as
// `shared:swarm:<holder>:<id>`.
import { agentRequest } from '../agents/agent-runtime.js'

const SIGN_IN_KINDS = new Set(['oauth', 'subscription'])
const RECEIVED = /^shared:swarm:([^:]+):(.+)$/
const HARNESS_SIGN_IN = /^pi:(.+)$/

// Every account in the fleet, or null when this runtime lists none.
export async function readFleetAccounts(tauri) {
  try {
    const answer = await agentRequest(tauri, 'local', 'fleet.credentials', {})
    // An older runtime lists no accounts and shows no holder.
    return Array.isArray(answer?.accounts) ? answer.accounts : null
  } catch (_) {
    return null
  }
}

// A sign-in that matters to the fleet: one another member holds, or one
// this machine holds and shares.
function signIn(accounts, named, match) {
  const account = accounts?.find((entry) => named(entry.name) && match(entry))
  if (!account || !SIGN_IN_KINDS.has(account.kind)) return null
  if (account.this_machine && !account.shared) return null
  return account
}

// The holder record of a router account row, by its id in the router's settings.
export function routerHold(accounts, id) {
  const received = RECEIVED.exec(id ?? '')
  const name = `router:${received ? received[2] : id}`
  const named = (entry) => entry === name
  if (received) return signIn(accounts, named, (entry) => entry.holder === received[1])
  return signIn(accounts, named, (entry) => entry.this_machine)
}

// The holder record of a Pi sign-in, by its provider id: this machine's own
// first, else the one another member shares here.
export function piHold(accounts, provider) {
  const named = (entry) => HARNESS_SIGN_IN.exec(entry ?? '')?.[1] === provider
  return signIn(accounts, named, (entry) => entry.this_machine) ?? signIn(accounts, named, (entry) => entry.usable_here)
}

// The one state line of a sign-in's holder.
export function holdLine(account, requested = false) {
  if (account.this_machine) return 'Signed in here'
  if (requested) return 'Moving the sign-in here.'
  return `Signed in on ${account.holder_name || 'another machine'}`
}

// The sign-ins other members hold that this machine cannot use yet, so no
// provider row shows them: each with its label, provider and one line. A
// shared one is on its way here. Hold sign-in here needs a sign-in that
// arrived, so these rows carry no control.
export function otherSignIns(accounts) {
  return (accounts ?? [])
    .filter((account) => SIGN_IN_KINDS.has(account.kind) && !account.this_machine && !account.usable_here)
    .map((account) => ({
      id: `${account.holder}/${account.name}`,
      label: account.label || account.name,
      provider: account.provider || account.family || '',
      line: `${holdLine(account)}${account.shared ? ', arriving' : ', not shared'}`,
    }))
}
