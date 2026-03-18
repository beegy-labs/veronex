-- 000008: N:N account-role mapping
--
-- Replaces the 1:1 accounts.role_id FK with a join table `account_roles`
-- so a single account can hold multiple roles.

-- ── Create join table ────────────────────────────────────────────────────────

CREATE TABLE account_roles (
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role_id    UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, role_id)
);

-- ── Migrate existing 1:1 assignments ─────────────────────────────────────────

INSERT INTO account_roles (account_id, role_id)
SELECT id, role_id FROM accounts WHERE role_id IS NOT NULL;

-- ── Drop old columns ─────────────────────────────────────────────────────────

ALTER TABLE accounts DROP COLUMN role_id;
ALTER TABLE accounts DROP COLUMN IF EXISTS role;
