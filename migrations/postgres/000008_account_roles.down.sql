-- Rollback: restore 1:1 role columns and drop join table

ALTER TABLE accounts ADD COLUMN role_id UUID REFERENCES roles(id);

UPDATE accounts a
SET role_id = (SELECT role_id FROM account_roles ar WHERE ar.account_id = a.id LIMIT 1);

ALTER TABLE accounts ADD COLUMN role VARCHAR(16) NOT NULL DEFAULT 'admin';

DROP TABLE account_roles;
