CREATE TABLE jobs (
  ticket          TEXT PRIMARY KEY NOT NULL,
  work_type       TEXT NOT NULL,
  state           TEXT NOT NULL,
  auto_merge      INTEGER NOT NULL DEFAULT 0,
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL
);

CREATE TABLE repo_tasks (
  id                  INTEGER PRIMARY KEY AUTOINCREMENT,
  ticket              TEXT NOT NULL REFERENCES jobs(ticket) ON DELETE CASCADE,
  org                 TEXT NOT NULL,
  repo                TEXT NOT NULL,
  branch              TEXT NOT NULL,
  worktree_path       TEXT NOT NULL,
  anchors_json        TEXT NOT NULL DEFAULT '[]',
  phase               TEXT NOT NULL,
  pr_url              TEXT,
  review_attempts     INTEGER NOT NULL DEFAULT 0,
  lint_test_attempts  INTEGER NOT NULL DEFAULT 0,
  ci_fix_attempts     INTEGER NOT NULL DEFAULT 0,
  UNIQUE (ticket, org, repo)
);

CREATE TABLE events (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  ticket      TEXT NOT NULL,
  kind        TEXT NOT NULL,
  payload     TEXT NOT NULL,
  ts          TEXT NOT NULL
);

CREATE INDEX events_by_ticket ON events(ticket, ts);

CREATE TABLE escalations (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  ticket        TEXT NOT NULL,
  repo_task_id  INTEGER,
  reason        TEXT NOT NULL,
  snapshot      TEXT NOT NULL,
  ts            TEXT NOT NULL
);
