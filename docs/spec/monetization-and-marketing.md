# Muniment — Monetization Strategy & Marketing Plan

**Status:** Working strategy v1 · July 2026
**Companions:** harness-spec.md (product), 01–05 design docs (surfaces)
**Ground truth:** Hosted SaaS, closed source. $15/seat/mo annual · $20/seat/mo monthly · 10-seat minimum · 30-day full trial. BYO provider keys or endpoints; muniment never marks up inference. No partner/MSP/reseller program. Direct sales only.

---

## 1. What we are actually selling

Not chat. Not tokens. Muniment sells **governed intelligence**: the layer where an org's identity, entitlements, routing policy, spend, and audit trail meet its AI usage. The buyer's alternative is not "another chat app," it's a governance gap: employees on personal ChatGPT accounts, unmanaged API keys in Slack DMs, no receipts.

Three value claims, in the order buyers feel them:

1. **Control:** groups grant, deny wins, the gateway enforces even if a client lies. Provider keys never touch a device.
2. **Receipts:** every answer carries its route, model, cost, and time. Append-only audit, SIEM export. The provenance line is user-visible compliance.
3. **Efficiency:** automatic routing sends easy work to cheap models. Because we never mark up inference, every routing win lands in the customer's budget, not ours. **The product partially pays for itself, and we can prove it with the customer's own receipts.**

The third claim is the commercial weapon. No competitor whose margin rides on tokens can make it.

## 2. Pricing rationale (decided; recorded for posterity)

- **$15/$20, per seat:** priced like infrastructure, not a utility. Cheap governance reads as unserious to security buyers. The 33% monthly premium pushes annual without punishing trials that convert mid-year.
- **10-seat minimum:** floor revenue of $1,800/org/yr clears support cost; filters hobbyists without a crippled free tier.
- **30-day full trial, every feature:** the trial is the demo. No feature gates — a governance product with a gated trial can't demonstrate governance.
- **No inference markup, ever:** the moat sentence. Third-party gateways and copilots monetize usage; our incentives point at reducing the customer's model bill. Repeat on every surface.
- **Anti-metering pledge:** flat per-seat, no usage meters, no surprise line items. Predictability is a feature for the buyer who signs.

### 2.1 Unit economics guardrails

Our COGS is control-plane compute + storage + support (no inference). Target gross margin ≥ 85%. Watch two abusers: audit-log storage growth (cap retention on the base plan at 12 months, offer extended retention as the first add-on) and workflow compute (fair-use policy written before launch, enforced by budgets we already built).

### 2.2 Expansion levers (roadmap, not launch)

In credibility order:
1. **Extended audit retention / compliance pack** (>12mo retention, legal hold, evidence exports). Natural, non-hypocritical, sells to the same buyer.
2. **Router intelligence subscription:** continuously tuned routing weights, per-vertical policies, quarterly savings reports. Monetizes the product's magic; renews on demonstrated savings. Trained only on opt-in aggregate telemetry, stated plainly.
3. **Premium identity:** advanced SCIM mappings, multi-IdP, SIEM streaming connectors.
4. **Enterprise plan (unadvertised):** SSO-enforced org policies, custom DPAs, security reviews, and the reserved self-host card — played only when a large deal demands it, never on the website.

What we never sell: per-message pricing, model markups, "priority routing," or paywalled security features. Each would contradict a public promise.

## 3. Market position

**Category:** the governed AI workspace. One sentence: *"One app for your whole org. Any model. Your rules. Every answer carries its receipt."*

| Against | Their story | Our wedge |
|---|---|---|
| ChatGPT Enterprise / Claude Team | One vendor's models, per-seat, opaque routing | Any model incl. yours; receipts; no vendor lock on intelligence |
| Open WebUI / LibreChat (self-managed) | Free, web-based, DIY ops | Desktop-native, zero ops, real RBAC UX, supported product |
| Gateways (LiteLLM cloud, OpenRouter, Portkey) | Infrastructure for developers | We're the workspace employees live in; gateway included, invisible |
| "AI governance" compliance vendors | Observe and report | We enforce at the gateway, not just dashboard it |

**ICP (beachhead):** 25–500 employee security-conscious orgs — security vendors, fintech, healthcare-adjacent, legal, MSP-like ops shops — with an existing IdP, real audit obligations, and current shadow-AI pain. Buyer: head of IT/security/platform. Champion: the internal AI lead (the "person like the founder" at every company).

**Why now:** every org just spent two years accumulating ungoverned AI usage; audits and insurance questionnaires are starting to ask about it; model plurality (open weights + multiple frontier vendors) makes single-vendor workspaces feel like lock-in.

## 4. Marketing plan

### 4.0 Positioning discipline

The word "sovereignty" appears once, on the homepage, ever. "AI" never appears in product copy. We market behaviors: routes, receipts, denies, records. The brand is the strategy — every screenshot of a provenance line is an ad no competitor can copy without rebuilding their business model.

### 4.1 Phase 0 — Foundation (pre-launch, 4–6 weeks)

- Site live per 04-marketing-site.md: landing, docs, pricing, security. The security page is a sales asset from day one.
- **Design-partner program, quiet:** 5–8 orgs from the founder's network, free through beta in exchange for weekly feedback and a case-study option. (Explicitly not a partner/reseller program — these are customers-in-waiting.)
- Instrument everything: trial start → IdP connected → 10th seat active → first policy published → first month's routing-savings figure. That last metric becomes the sales deck.
- Waitlist with one question: "What are you using today, and who's allowed to see the logs?" — segmentation and copy research in one field.

### 4.2 Phase 1 — Launch (weeks 0–8)

- **The demo is the receipt.** Launch asset: a 90-second recording — a question is asked, the ring thinks, the answer streams, the provenance line lands, the admin's audit explorer shows the same event. No voiceover hype; mono captions.
- Launch surfaces: Hacker News (Show HN, founder-voice, technical honesty about what it is and isn't), Product Hunt same week, founder's own podcast/network circuit — the host becomes the guest for a season on other security/MSP/IT shows.
- **Founding-customer offer:** first 20 orgs lock $12/seat annual for two years. Scarcity that's real (a number), not theatrical (a countdown).
- PR angle for trade press: not "new AI app" but "shadow AI is an audit finding now" — sell the problem category.

### 4.3 Phase 2 — Engine (months 2–12)

Content strategy: **publish the ledger.** Everything we write is receipts-flavored:
- *The Routing Report* (quarterly): anonymized aggregate — what % of org AI requests actually need a frontier model, cost curves by task class. Original data nobody else can publish honestly; every trade journalist's citation.
- Teardowns: "What your ChatGPT Enterprise audit log actually contains (and doesn't)." Comparison pages per competitor, factual, mono tables.
- Buyer enablement: a genuinely good "AI usage policy" template, an internal-rollout playbook, a security-questionnaire answer pack. Gated by nothing; the docs are the funnel.
- SEO spine: "AI audit trail," "AI usage policy enforcement," "route LLM requests by policy," "ChatGPT Enterprise alternative multi-model." Low volume, near-perfect intent.
- One channel done excellently over five done adequately: pick LinkedIn + the founder's podcast circuit (where the audience already is), skip paid until organic signal exists.

### 4.4 Sales motion

Product-led trial with founder-led close. Trial → automated day-3 "connect your IdP" nudge → day-14 "your routing savings so far: $X" email (the product sells itself with its own receipts) → day-25 founder call for ≥25-seat orgs. Annual invoice, card for monthly. No SDRs, no demo-gate, no MQL theater until >$1M ARR forces process.

### 4.5 Metrics that matter

- Trial → paid conversion (target ≥ 12% of orgs that connect an IdP)
- Time-to-first-published-policy (activation proxy; target < 3 days)
- Seat expansion at renewal (target net revenue retention ≥ 110%)
- Routing savings delivered vs. subscription cost (the ratio we publish when it's flattering; the product thesis is falsifiable and we should know first)
- Logo churn reasons, verbatim, reviewed monthly

## 5. Risks, named

- **Platform absorption:** OpenAI/Anthropic ship better org controls. Defense: multi-model neutrality is structural for us and impossible for them; deepen routing + receipts.
- **Hosted-only objection from our own ICP:** some security buyers refuse SaaS for this layer. Defense: the unadvertised enterprise self-host card (§2.2.4); track how often it's demanded — if >30% of qualified pipeline, revisit the public stance with real data.
- **Trial abuse / support load at 10 seats:** floor pricing helps; fair-use and retention caps written before launch, not after the first incident.
- **The router underdelivers savings:** then claim #3 quietly retires and we sell control + receipts alone — still a product, weaker wedge. The eval harness (harness-spec §5.2) exists to know early.

## 6. First 90 days, one line each

Weeks 1–2 site + instrumentation live · Weeks 2–6 design partners onboarded, weekly cadence · Week 6 pricing page public · Week 8 launch (HN + PH + circuit) · Weeks 8–13 daily trial-funnel review, founder closes everything, one Routing Report teaser published · Day 90: decide from data whether the hosted-only stance holds.
