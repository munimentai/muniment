# Journal-idea notes — raw material for the muniment.ai journal

Owner-directed 2026-07-14 (cross-lane content pipeline). When a merged
change ships something a practitioner would want to read about, add a
short note here as `docs/journal-ideas/<slug>.md` — in the same PR or a
docs-only follow-up. Three kinds of material are wanted:

- a new feature or concept worth explaining publicly,
- a hard-won engineering lesson from building this product,
- a pattern in what agents struggle with (and what fixed it).

A merged note is picked up automatically and lands with the site lane
(muniment.ai) as an idea ticket carrying this file's FULL TEXT and
nothing else. The site lane decides whether a public journal article is
warranted; most notes will not become articles, and that is fine.

Rules for a note:

1. **Stand alone.** The site lane sees only this file's text, never this
   repository. Include the concrete evidence (numbers, before/after,
   what failed and why) and enough context for an outside writer.
2. **Assume near-verbatim publication.** Never include secrets,
   credentials, security-sensitive implementation detail, customer
   data, or anything that must not be public. If a lesson cannot be
   told safely, do not write the note.
3. **One idea per note.** Slug = a short kebab-case name for the idea.
4. **Fire and forget.** Do not track article status here; never edit a
   merged note to "update" the site (a re-merge re-files the idea). New
   learning = new note.
