-- ENSIP-30 stealth gateway (src/ens_gateway.rs). One row per address handed out, so the
-- recipient can find payments to it. Portable SQLite/Postgres (TEXT and BIGINT only).
-- name_key = HMAC(identity key, "lowki/ens-name/v1" || name): a dump alone does not map names
-- to addresses. Never stored: the ephemeral private key, the requester's IP.
CREATE TABLE IF NOT EXISTS ens_stealth_records (
    stealth_address TEXT PRIMARY KEY,
    name_key TEXT NOT NULL,
    coin_type BIGINT NOT NULL,
    ephemeral_pubkey TEXT NOT NULL,
    view_tag BIGINT NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS ens_stealth_records_name ON ens_stealth_records(name_key, created_at);
