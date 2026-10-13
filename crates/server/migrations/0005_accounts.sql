-- Adresse e-mail vérifiée (lien reçu par e-mail, réinitialisation ou invitation acceptée).
ALTER TABLE users ADD COLUMN email_verified_at TIMESTAMPTZ;

-- Jetons à usage unique envoyés par e-mail : seul leur hash SHA-256 est stocké.
--   reset  : réinitialisation du mot de passe (30 min)
--   verify : vérification de l'adresse (24 h)
--   invite : invitation dans un workspace (7 jours)
CREATE TABLE email_tokens (
    id               UUID PRIMARY KEY,
    token_hash       BYTEA NOT NULL UNIQUE,
    purpose          TEXT NOT NULL CHECK (purpose IN ('reset', 'verify', 'invite')),
    email_normalized TEXT NOT NULL,
    user_id          UUID REFERENCES users (id) ON DELETE CASCADE,
    workspace_id     UUID REFERENCES workspaces (id) ON DELETE CASCADE,
    role             TEXT CHECK (role IN ('owner', 'member')),
    invited_by       UUID REFERENCES users (id) ON DELETE CASCADE,
    -- Invitation envoyée par e-mail : l'accepter prouve la possession de l'adresse.
    delivered        BOOLEAN NOT NULL DEFAULT false,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at       TIMESTAMPTZ NOT NULL,
    CHECK (purpose <> 'invite' OR (workspace_id IS NOT NULL AND role IS NOT NULL))
);
CREATE INDEX email_tokens_user_idx ON email_tokens (user_id);
CREATE INDEX email_tokens_expires_idx ON email_tokens (expires_at);
-- Une invitation en attente par adresse et par workspace.
CREATE UNIQUE INDEX email_tokens_invite_idx ON email_tokens (workspace_id, email_normalized) WHERE purpose = 'invite';
