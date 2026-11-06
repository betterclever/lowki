-- Portable schema for SQLite and Postgres (TEXT and BIGINT only; times are unix seconds).
-- Binary values are stored as base64url text.

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    webauthn_handle TEXT NOT NULL UNIQUE,
    -- Per-user salt for pairwise app IDs (hex). Useless without LOWKI_SERVER_SECRET.
    pairwise_salt TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

-- Sign-in methods. subject_key is HMAC(server_secret, "<provider>:" || subject) for OIDC/email,
-- and "passkey:" || credential_id for passkeys. No plaintext email unless LOWKI_STORE_EMAIL=true.
CREATE TABLE IF NOT EXISTS identities (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    provider TEXT NOT NULL,
    subject_key TEXT NOT NULL UNIQUE,
    email TEXT,
    created_at BIGINT NOT NULL,
    last_used_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS identities_user ON identities(user_id);

CREATE TABLE IF NOT EXISTS passkeys (
    cred_id TEXT PRIMARY KEY,
    identity_id TEXT NOT NULL UNIQUE REFERENCES identities(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    passkey_json TEXT NOT NULL,
    -- Passkey-wrapped seed blob (opaque; useless without the PRF output).
    wrapped_blob TEXT,
    created_at BIGINT NOT NULL,
    last_used_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS passkeys_user ON passkeys(user_id);

CREATE TABLE IF NOT EXISTS webauthn_flows (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    state_json TEXT NOT NULL,
    user_id TEXT,
    user_handle TEXT,
    expires_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS oauth_flows (
    state_hash TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    binding_hash TEXT NOT NULL,
    code_verifier TEXT NOT NULL,
    nonce TEXT NOT NULL,
    link_user_id TEXT,
    expires_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS otp_codes (
    email_key TEXT PRIMARY KEY,
    code_mac TEXT NOT NULL,
    attempts BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id_hash TEXT PRIMARY KEY,
    refresh_hash TEXT NOT NULL UNIQUE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- JSON list of {identity_id, kind, at}: every sign-in performed in this session.
    auths TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    refresh_expires_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_user ON sessions(user_id);

-- Server share S (opaque bytes), released only to a session authenticated with bound_identity_id.
CREATE TABLE IF NOT EXISTS shares (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    share TEXT NOT NULL,
    bound_identity_id TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS recovery_requests (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    requested_at BIGINT NOT NULL,
    available_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS apps (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    allowed_origins TEXT NOT NULL,
    secret_hash TEXT NOT NULL,
    webhook_url TEXT,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_users (
    app_id TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    pairwise_id TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL,
    last_seen_at BIGINT NOT NULL,
    PRIMARY KEY (app_id, pairwise_id)
);
CREATE INDEX IF NOT EXISTS app_users_user ON app_users(user_id);

CREATE TABLE IF NOT EXISTS signing_keys (
    kid TEXT PRIMARY KEY,
    -- PKCS#8 private key sealed with a key derived from LOWKI_SERVER_SECRET.
    private_sealed TEXT NOT NULL,
    public_jwk TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    retired_at BIGINT
);

CREATE TABLE IF NOT EXISTS audit_log (
    id TEXT PRIMARY KEY,
    at BIGINT NOT NULL,
    user_id TEXT,
    event TEXT NOT NULL,
    detail TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS audit_log_at ON audit_log(at);

CREATE TABLE IF NOT EXISTS webhook_events (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL,
    event TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    delivered_at BIGINT,
    attempts BIGINT NOT NULL
);
