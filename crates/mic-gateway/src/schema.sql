CREATE TABLE web_admin (
  id INTEGER PRIMARY KEY CHECK(id = 1),
  password_hash TEXT NOT NULL,
  session_key BLOB NOT NULL CHECK(length(session_key) = 32)
);

CREATE TABLE web_push_keys (
  id INTEGER PRIMARY KEY CHECK(id = 1),
  private_key BLOB NOT NULL,
  public_key BLOB NOT NULL
);

CREATE TABLE web_push_subscriptions (
  endpoint TEXT PRIMARY KEY,
  p256dh BLOB NOT NULL CHECK(length(p256dh) = 65),
  auth BLOB NOT NULL CHECK(length(auth) = 16),
  created_at INTEGER NOT NULL
);
