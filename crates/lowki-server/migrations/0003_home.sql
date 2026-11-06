-- Wallet home (src/app_list.rs, src/ens_names.rs). Portable SQLite/Postgres.

-- The user's app list, encrypted on the device under a key derived from the seed
-- ("lowki/app-list/key/v1" in lowki-worker). The server sees only an opaque blob and its size.
-- version: optimistic concurrency (two app iframes adding at once).
CREATE TABLE IF NOT EXISTS app_lists (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    blob TEXT NOT NULL,
    version BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- Sponsored <label>.<parent> names: one per user. The operator pays for registration and the
-- owner batch, so it knows which user claimed which (public) name and owner address.
-- status: registering | registered | ready | failed
CREATE TABLE IF NOT EXISTS ens_names (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    label TEXT NOT NULL UNIQUE,
    owner TEXT NOT NULL,
    status TEXT NOT NULL,
    register_tx TEXT,
    setup_tx TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
