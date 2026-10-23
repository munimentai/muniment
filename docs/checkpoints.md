# Turn checkpoints

`muniment_core::checkpoint` records a thread's working folder around each
reply, so Review changes can show and revert a turn's changes and Rewind can
return the folder and the conversation to before a message. It has no Tauri
or network dependency.

## Records

Before a reply sends its prompt, the runtime records the run's folder and the
thread's Pi session file. After the reply ends, in any state, it records the
folder again. A resumed reply records only its end. Compaction records
nothing. A record lists each file by its `/`-separated path with its SHA-256,
size, modification time and execute bit, every folder, the skipped paths with
their reason, and the excluded folders it met.

The store lives at `<profile>/checkpoints`: objects in a `LocalCas` there and
one JSON record per run and kind under `records/<thread id>/`. A record of a
file whose size and time match the folder's previous record, and which
changed at least two seconds before it, reuses that hash without reading the
file.

## Bounds

- 20,000 files, 16 MB per file and 512 MB of newly read bytes per record, and
  5 seconds per record. A larger file is named as too large. A folder past
  another bound gives an incomplete record, which restores no file.
- `.git`, `.hg`, `.svn`, `node_modules`, `target`, `.venv` and `__pycache__`
  are left out by name, and the runtime's profile and state folders by path.
  Links are named, not followed.
- Each thread keeps its newest 200 records for 30 days, and the store keeps
  at most 4 GB of objects, dropping the oldest records first. Every twentieth
  end record sweeps the records and removes the objects no record names.

## Restore

`plan_restore` compares a record with the folder now: files to write back,
files made since that go, files the record could not keep, and the excluded
folders. Its token names that exact plan. `restore` refuses another token,
records the folder as it is first, removes the new files, writes each file
through a temporary file beside it, reads it back and checks its hash. A path
whose folder became a link or a file fails instead of writing outside the
folder. The report is complete only when every planned file was written and
verified and nothing the record holds was left out.

## Rewind

`thread.rewind` takes the store's lock, refuses while a reply runs in the
thread or a run in another thread uses an overlapping folder, restores the
files when the request carries the preview's token, appends `thread.rewound`
with the run and every later run of the thread, and writes the saved
conversation back. History, the conversation each next run reopens and the
thread's recent text skip rewound runs. Their events and receipts stay, so
usage still counts them. A rewind of a turn whose conversation was not saved
is refused.

## Turn changes

`run.changes` compares a run's two records and answers each changed file with
its counts, its state now (applied, reverted or changed since) and a line
diff of text files up to 1 MB a side, at most 200 files and 512 KB of diff.
`run.revert` writes one file back, or one hunk when the turn's lines for it
still stand in the file, and refuses a file that changed after the turn.
