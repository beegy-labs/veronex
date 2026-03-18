-- Rollback: remove role_id from accounts and drop roles table
ALTER TABLE accounts DROP COLUMN IF EXISTS role_id;
DROP TABLE IF EXISTS roles;
