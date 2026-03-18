-- 000007: Role-based permission system
--
-- Adds a `roles` table and links accounts to roles via `role_id`.
-- Backward-compatible: existing `role` TEXT column is kept for reads
-- during the transition period (old JWTs, old code paths).

-- ── Create roles table ──────────────────────────────────────────────────────

CREATE TABLE roles (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name        VARCHAR(64) NOT NULL UNIQUE,
    permissions TEXT[]      NOT NULL DEFAULT '{}',
    menus       TEXT[]      NOT NULL DEFAULT '{}',
    is_system   BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── Seed built-in roles ─────────────────────────────────────────────────────

INSERT INTO roles (name, permissions, menus, is_system) VALUES (
    'super',
    ARRAY['dashboard_view','api_test','provider_manage','key_manage','account_manage','audit_view','settings_manage','role_manage'],
    ARRAY['dashboard','flow','jobs','performance','usage','test','providers','servers','keys','accounts','audit','api_docs'],
    TRUE
);

INSERT INTO roles (name, permissions, menus, is_system) VALUES (
    'viewer',
    ARRAY['dashboard_view'],
    ARRAY['dashboard','flow','jobs','performance','usage','api_docs'],
    TRUE
);

-- ── Add role_id to accounts ─────────────────────────────────────────────────

ALTER TABLE accounts ADD COLUMN role_id UUID REFERENCES roles(id);

-- Migrate existing accounts based on their text role column
UPDATE accounts SET role_id = (SELECT id FROM roles WHERE name = 'super')  WHERE role = 'super';
UPDATE accounts SET role_id = (SELECT id FROM roles WHERE name = 'viewer') WHERE role = 'admin' AND role_id IS NULL;

-- Make role_id NOT NULL after migration
ALTER TABLE accounts ALTER COLUMN role_id SET NOT NULL;
